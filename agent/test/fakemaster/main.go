// fakemaster 是测试用的假主控，只用于在测试 VPS 上验证 op-agent，不随 Agent 发布。
//
// 它在本机回环地址上接受 Agent 的 WebSocket 连接（op-agent 的 MASTER_URL 写 http://127.0.0.1:<端口>），
// 回 HelloResult、推送期望状态，把 Agent 发来的每条消息按一行 JSON 打到标准输出。
// 控制接口（用 curl 调）：
//
//	POST /ctl/state             请求体是 JSON 格式的 DesiredState，版本号自动填，推送给 Agent
//	POST /ctl/request?timeout=  请求体是 JSON 格式的 MasterMessage（只填 body），作为请求发给 Agent，返回它的回复
//	POST /ctl/upgrade?version=&arch=&bad=
//	                            按 -binary-dir 里的文件算 SHA-256、用 -sign-key 签名，给 Agent 发 Upgrade，返回它的回复；
//	                            bad=sha256 或 bad=signature 故意发错的值
//
// 另外按 -binary-dir 提供 GET /api/agent/binary/{版本}/{架构}（要带 Authorization: Bearer <Token>），升级测试用。
// fakemaster -keygen 生成一对 Ed25519 密钥（标准 base64），公钥编译进 op-agent，私钥给 -sign-key。
package main

import (
	"context"
	"crypto/ecdsa"
	"crypto/ed25519"
	"crypto/elliptic"
	"crypto/rand"
	"crypto/sha256"
	"crypto/tls"
	"crypto/x509"
	"crypto/x509/pkix"
	"encoding/base64"
	"encoding/pem"
	"errors"
	"flag"
	"fmt"
	"io"
	"log/slog"
	"math/big"
	"net/http"
	"os"
	"path/filepath"
	"sync"
	"sync/atomic"
	"time"

	"github.com/coder/websocket"
	"google.golang.org/protobuf/encoding/protojson"
	"google.golang.org/protobuf/proto"

	agentv1 "github.com/yuuuki-creation/open-proxy/agent/internal/pb/openproxy/agent/v1"
)

func main() {
	listen := flag.String("listen", "127.0.0.1:21000", "监听地址，只用本机回环地址")
	statePath := flag.String("state", "", "连上后推送的期望状态（JSON 格式的 DesiredState），不填就不推")
	token := flag.String("token", "", "Agent 的 Token；不填就接受任何 Token")
	helloStatus := flag.String("hello-status", "ok", "回给 Hello 的结果：ok、invalid-token、duplicate、deleted")
	masterVersion := flag.String("master-version", "", "主控版本；不填就和 Agent 一样（同步期望状态）")
	binaryDir := flag.String("binary-dir", "", "升级用的二进制目录，文件名 op-agent-<版本>-<架构>")
	signKey := flag.String("sign-key", "", "给升级包签名的 Ed25519 私钥（-keygen 生成的）")
	rejectVersion := flag.String("reject-version", "", "这个版本的 Agent 来连时回「Token 无效」，模拟新版本连不上")
	keygen := flag.Bool("keygen", false, "生成一对 Ed25519 密钥后退出")
	tlsListen := flag.String("tls-listen", "", "再开一个只做握手的测试 TLS 服务（TLS 1.3、ALPN h2、自签证书），REALITY 检测和扫描的测试用")
	tlsName := flag.String("tls-name", "reality-test.example.com", "测试 TLS 服务证书里的域名")
	flag.Parse()

	if *keygen {
		pub, priv, err := ed25519.GenerateKey(rand.Reader)
		if err != nil {
			fmt.Fprintln(os.Stderr, err)
			os.Exit(1)
		}
		fmt.Printf("PRIVATE=%s\nPUBLIC=%s\n", base64.StdEncoding.EncodeToString(priv.Seed()), base64.StdEncoding.EncodeToString(pub))
		return
	}

	slog.SetDefault(slog.New(slog.NewTextHandler(os.Stderr, nil)))
	m := &master{
		token:         *token,
		helloStatus:   *helloStatus,
		masterVersion: *masterVersion,
		binaryDir:     *binaryDir,
		rejectVersion: *rejectVersion,
		pending:       make(map[uint64]chan *agentv1.AgentMessage),
	}
	if *signKey != "" {
		seed, err := base64.StdEncoding.DecodeString(*signKey)
		if err != nil || len(seed) != ed25519.SeedSize {
			slog.Error("-sign-key 不是 -keygen 生成的私钥")
			os.Exit(1)
		}
		m.signKey = ed25519.NewKeyFromSeed(seed)
	}
	if *statePath != "" {
		data, err := os.ReadFile(*statePath)
		if err != nil {
			slog.Error("读期望状态文件", "err", err)
			os.Exit(1)
		}
		ds := &agentv1.DesiredState{}
		if err := protojson.Unmarshal(data, ds); err != nil {
			slog.Error("解析期望状态文件", "err", err)
			os.Exit(1)
		}
		m.state = ds
	}

	if *tlsListen != "" {
		if err := serveTLS(*tlsListen, *tlsName); err != nil {
			slog.Error("开测试 TLS 服务", "err", err)
			os.Exit(1)
		}
	}

	mux := http.NewServeMux()
	mux.HandleFunc("/api/agent/ws", m.serveWS)
	mux.HandleFunc("POST /ctl/state", m.serveState)
	mux.HandleFunc("POST /ctl/request", m.serveRequest)
	mux.HandleFunc("POST /ctl/upgrade", m.serveUpgrade)
	mux.HandleFunc("GET /api/agent/binary/{version}/{arch}", m.serveBinary)
	slog.Info("假主控启动", "listen", *listen)
	if err := http.ListenAndServe(*listen, mux); err != nil {
		slog.Error("退出", "err", err)
		os.Exit(1)
	}
}

type master struct {
	token         string
	helloStatus   string
	masterVersion string
	binaryDir     string
	rejectVersion string
	signKey       ed25519.PrivateKey

	mu      sync.Mutex
	conn    *websocket.Conn // 当前的 Agent 连接；没有时为 nil
	state   *agentv1.DesiredState
	version uint64               // 上次推送的期望状态版本
	cert    *agentv1.Certificate // 自己生成的自签证书
	nextID  atomic.Uint64
	pending map[uint64]chan *agentv1.AgentMessage
}

func (m *master) serveWS(w http.ResponseWriter, r *http.Request) {
	conn, err := websocket.Accept(w, r, &websocket.AcceptOptions{CompressionMode: websocket.CompressionDisabled})
	if err != nil {
		slog.Warn("WebSocket 握手失败", "err", err)
		return
	}
	defer conn.CloseNow()
	conn.SetReadLimit(4 << 20)
	ctx := r.Context()

	helloDone := false
	for {
		_, data, err := conn.Read(ctx)
		if err != nil {
			slog.Info("Agent 断开", "err", err)
			m.mu.Lock()
			if m.conn == conn {
				m.conn = nil
			}
			m.mu.Unlock()
			return
		}
		msg := &agentv1.AgentMessage{}
		if err := proto.Unmarshal(data, msg); err != nil {
			slog.Warn("解码失败", "err", err)
			continue
		}
		printMessage(msg)

		if hello := msg.GetHello(); hello != nil && !helloDone {
			helloDone = true
			if !m.handleHello(ctx, conn, msg.GetId(), hello) {
				return
			}
			continue
		}
		if msg.GetReplyTo() != 0 {
			m.mu.Lock()
			ch := m.pending[msg.GetReplyTo()]
			delete(m.pending, msg.GetReplyTo())
			m.mu.Unlock()
			if ch != nil {
				ch <- msg
			}
		}
	}
}

// handleHello 回 HelloResult；成功时登记连接并推送期望状态。返回 false 表示要断开。
func (m *master) handleHello(ctx context.Context, conn *websocket.Conn, id uint64, hello *agentv1.Hello) bool {
	status := map[string]agentv1.HelloResult_Status{
		"ok":            agentv1.HelloResult_STATUS_OK,
		"invalid-token": agentv1.HelloResult_STATUS_INVALID_TOKEN,
		"duplicate":     agentv1.HelloResult_STATUS_DUPLICATE_CONNECTION,
		"deleted":       agentv1.HelloResult_STATUS_SERVER_DELETED,
	}[m.helloStatus]
	if m.token != "" && hello.GetToken() != m.token {
		status = agentv1.HelloResult_STATUS_INVALID_TOKEN
	}
	if m.rejectVersion != "" && hello.GetAgentVersion() == m.rejectVersion {
		slog.Info("按 -reject-version 拒绝这个版本的 Agent", "version", hello.GetAgentVersion())
		status = agentv1.HelloResult_STATUS_INVALID_TOKEN
	}
	version := m.masterVersion
	if version == "" {
		version = hello.GetAgentVersion()
	}
	synced := version == hello.GetAgentVersion()
	result := &agentv1.MasterMessage{
		Id:      m.nextID.Add(1),
		ReplyTo: id,
		Body: &agentv1.MasterMessage_HelloResult{HelloResult: &agentv1.HelloResult{
			Status: status, MasterVersion: version, Sync: synced,
		}},
	}
	if err := write(ctx, conn, result); err != nil {
		slog.Warn("回 HelloResult 失败", "err", err)
		return false
	}
	if status != agentv1.HelloResult_STATUS_OK {
		// 和主控一样：认证不通过就关连接
		conn.Close(websocket.StatusPolicyViolation, "认证不通过")
		return false
	}
	m.mu.Lock()
	m.conn = conn
	state := m.state
	m.mu.Unlock()
	if synced && state != nil {
		if _, err := m.push(ctx, state); err != nil {
			slog.Warn("推送期望状态失败", "err", err)
		}
	}
	return true
}

// errNoAgent 表示现在没有 Agent 连着。
var errNoAgent = errors.New("没有 Agent 连着")

// push 填上新的版本号，推送期望状态，返回版本号。没带证书时填上假主控自己生成的自签证书
// （TLS 节点要用；每次都是同一张，免得每次推送都重建 TLS 节点）。
func (m *master) push(ctx context.Context, ds *agentv1.DesiredState) (uint64, error) {
	m.mu.Lock()
	conn := m.conn
	m.version = max(m.version+1, uint64(time.Now().UnixMilli()))
	ds = proto.Clone(ds).(*agentv1.DesiredState)
	ds.Version = m.version
	if ds.Certificate == nil {
		if m.cert == nil {
			cert, err := selfSigned("localhost")
			if err != nil {
				m.mu.Unlock()
				return 0, err
			}
			m.cert = cert
		}
		ds.Certificate = m.cert
	}
	m.state = ds
	m.mu.Unlock()
	if conn == nil {
		return 0, errNoAgent
	}
	slog.Info("推送期望状态", "version", ds.GetVersion())
	return ds.GetVersion(), write(ctx, conn, &agentv1.MasterMessage{
		Id:   m.nextID.Add(1),
		Body: &agentv1.MasterMessage_DesiredState{DesiredState: ds},
	})
}

func (m *master) serveState(w http.ResponseWriter, r *http.Request) {
	data, err := io.ReadAll(r.Body)
	if err != nil {
		http.Error(w, err.Error(), http.StatusBadRequest)
		return
	}
	ds := &agentv1.DesiredState{}
	if err := protojson.Unmarshal(data, ds); err != nil {
		http.Error(w, "解析期望状态: "+err.Error(), http.StatusBadRequest)
		return
	}
	version, err := m.push(r.Context(), ds)
	if err != nil {
		http.Error(w, err.Error(), http.StatusServiceUnavailable)
		return
	}
	fmt.Fprintf(w, "已推送，版本 %d\n", version)
}

func (m *master) serveRequest(w http.ResponseWriter, r *http.Request) {
	timeout := time.Minute
	if s := r.URL.Query().Get("timeout"); s != "" {
		d, err := time.ParseDuration(s)
		if err != nil {
			http.Error(w, err.Error(), http.StatusBadRequest)
			return
		}
		timeout = d
	}
	data, err := io.ReadAll(r.Body)
	if err != nil {
		http.Error(w, err.Error(), http.StatusBadRequest)
		return
	}
	msg := &agentv1.MasterMessage{}
	if err := protojson.Unmarshal(data, msg); err != nil {
		http.Error(w, "解析请求: "+err.Error(), http.StatusBadRequest)
		return
	}
	reply, err := m.request(r.Context(), msg, timeout)
	if err != nil {
		http.Error(w, err.Error(), http.StatusGatewayTimeout)
		return
	}
	out, _ := protojson.MarshalOptions{Multiline: true}.Marshal(reply)
	w.Write(append(out, '\n'))
}

// request 发一个请求，等 Agent 回复。
func (m *master) request(ctx context.Context, msg *agentv1.MasterMessage, timeout time.Duration) (*agentv1.AgentMessage, error) {
	id := m.nextID.Add(1)
	msg.Id = id
	ch := make(chan *agentv1.AgentMessage, 1)
	m.mu.Lock()
	conn := m.conn
	m.pending[id] = ch
	m.mu.Unlock()
	defer func() {
		m.mu.Lock()
		delete(m.pending, id)
		m.mu.Unlock()
	}()
	if conn == nil {
		return nil, errNoAgent
	}
	if err := write(ctx, conn, msg); err != nil {
		return nil, err
	}
	ctx, cancel := context.WithTimeout(ctx, timeout)
	defer cancel()
	select {
	case reply := <-ch:
		return reply, nil
	case <-ctx.Done():
		return nil, fmt.Errorf("等回复: %w", ctx.Err())
	}
}

// serveUpgrade 按 -binary-dir 里的文件拼一个 Upgrade 请求发给 Agent，返回它的回复。
func (m *master) serveUpgrade(w http.ResponseWriter, r *http.Request) {
	q := r.URL.Query()
	version, arch := q.Get("version"), q.Get("arch")
	if m.signKey == nil || m.binaryDir == "" {
		http.Error(w, "没有配置 -sign-key 或 -binary-dir", http.StatusBadRequest)
		return
	}
	name := "op-agent-" + version + "-" + arch
	if version == "" || arch == "" || filepath.Base(name) != name {
		http.Error(w, "version、arch 不对", http.StatusBadRequest)
		return
	}
	content, err := os.ReadFile(filepath.Join(m.binaryDir, name))
	if err != nil {
		// 文件不存在时照样发请求（用假的值），测试下载失败
		slog.Warn("读二进制失败，用假的 SHA-256 和签名", "err", err)
		content = []byte("没有这个文件")
	}
	sum := sha256.Sum256(content)
	sig := ed25519.Sign(m.signKey, content)
	switch q.Get("bad") {
	case "sha256":
		sum[0] ^= 0xff
	case "signature":
		sig[0] ^= 0xff
	}
	msg := &agentv1.MasterMessage{Body: &agentv1.MasterMessage_Upgrade{Upgrade: &agentv1.Upgrade{
		Version:      version,
		DownloadPath: "/api/agent/binary/" + version + "/" + arch,
		Sha256:       sum[:],
		Signature:    sig,
	}}}
	reply, err := m.request(r.Context(), msg, 5*time.Minute)
	if err != nil {
		http.Error(w, err.Error(), http.StatusGatewayTimeout)
		return
	}
	out, _ := protojson.MarshalOptions{Multiline: true}.Marshal(reply)
	w.Write(append(out, '\n'))
}

func (m *master) serveBinary(w http.ResponseWriter, r *http.Request) {
	if m.binaryDir == "" {
		http.NotFound(w, r)
		return
	}
	if m.token != "" && r.Header.Get("Authorization") != "Bearer "+m.token {
		slog.Warn("下载二进制的请求没带对 Token", "path", r.URL.Path)
		http.Error(w, "Token 不对", http.StatusUnauthorized)
		return
	}
	name := "op-agent-" + r.PathValue("version") + "-" + r.PathValue("arch")
	if filepath.Base(name) != name {
		http.NotFound(w, r)
		return
	}
	slog.Info("Agent 下载二进制", "file", name)
	http.ServeFile(w, r, filepath.Join(m.binaryDir, name))
}

func write(ctx context.Context, conn *websocket.Conn, msg *agentv1.MasterMessage) error {
	data, err := proto.Marshal(msg)
	if err != nil {
		return err
	}
	ctx, cancel := context.WithTimeout(ctx, 10*time.Second)
	defer cancel()
	return conn.Write(ctx, websocket.MessageBinary, data)
}

// serveTLS 在 addr 上开一个只做 TLS 握手的服务：TLS 1.3、ALPN h2、name 的自签证书，握手完就关连接。
func serveTLS(addr, name string) error {
	pemCert, err := selfSigned(name)
	if err != nil {
		return err
	}
	cert, err := tls.X509KeyPair([]byte(pemCert.GetCertPem()), []byte(pemCert.GetKeyPem()))
	if err != nil {
		return err
	}
	l, err := tls.Listen("tcp", addr, &tls.Config{
		Certificates: []tls.Certificate{cert},
		MinVersion:   tls.VersionTLS13,
		NextProtos:   []string{"h2"},
	})
	if err != nil {
		return err
	}
	slog.Info("测试 TLS 服务启动", "listen", addr, "name", name)
	go func() {
		for {
			conn, err := l.Accept()
			if err != nil {
				slog.Error("测试 TLS 服务退出", "err", err)
				return
			}
			go func() {
				defer conn.Close()
				conn.SetDeadline(time.Now().Add(10 * time.Second))
				if tc, ok := conn.(*tls.Conn); ok {
					tc.Handshake()
				}
			}()
		}
	}()
	return nil
}

// selfSigned 生成一张给 name 的自签证书（ECDSA P-256，有效 7 天）。
func selfSigned(name string) (*agentv1.Certificate, error) {
	key, err := ecdsa.GenerateKey(elliptic.P256(), rand.Reader)
	if err != nil {
		return nil, fmt.Errorf("生成私钥: %w", err)
	}
	template := &x509.Certificate{
		SerialNumber: big.NewInt(time.Now().UnixNano()),
		Subject:      pkix.Name{CommonName: name},
		DNSNames:     []string{name},
		NotBefore:    time.Now().Add(-time.Hour),
		NotAfter:     time.Now().Add(7 * 24 * time.Hour),
		KeyUsage:     x509.KeyUsageDigitalSignature,
		ExtKeyUsage:  []x509.ExtKeyUsage{x509.ExtKeyUsageServerAuth},
	}
	der, err := x509.CreateCertificate(rand.Reader, template, template, &key.PublicKey, key)
	if err != nil {
		return nil, fmt.Errorf("生成证书: %w", err)
	}
	keyDER, err := x509.MarshalPKCS8PrivateKey(key)
	if err != nil {
		return nil, fmt.Errorf("编码私钥: %w", err)
	}
	return &agentv1.Certificate{
		CertPem: string(pem.EncodeToMemory(&pem.Block{Type: "CERTIFICATE", Bytes: der})),
		KeyPem:  string(pem.EncodeToMemory(&pem.Block{Type: "PRIVATE KEY", Bytes: keyDER})),
	}, nil
}

// printMessage 把 Agent 发来的消息打成一行 JSON，前面带时间。
func printMessage(msg *agentv1.AgentMessage) {
	out, err := protojson.Marshal(msg)
	if err != nil {
		slog.Warn("转 JSON 失败", "err", err)
		return
	}
	fmt.Printf("%s %s\n", time.Now().Format("15:04:05.000"), out)
}

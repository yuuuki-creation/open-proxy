// proxyclient 是测试用的代理客户端，只用于在测试 VPS 上验证 op-agent 的节点，不随 Agent 发布。
//
// 支持五种协议：shadowsocks（2022）、vless（REALITY）、hysteria2、anytls 用内嵌的 sing-box 当客户端，
// mieru 用 mieru 的 apis/client。经节点做一件事，打一行结果（成功或失败、耗时、说明），失败时退出码为 1：
//
//	-tcp-echo 地址   连回显服务，发一行字等它原样回来
//	-http URL        发一个 HTTP GET（只支持 http://）
//	-udp-echo 地址   发一个 UDP 包等回显
//	-dns 地址        发一个 DNS 查询（-name 指定域名）；同时给了 -udp-echo 时，两个包在同一个 UDP 会话里先后发
//
// 另外两个模式不连节点：-echo-server 地址（TCP 和 UDP 回显服务，每收到一个 UDP 包打一行）、
// -reality-keygen（生成一对 REALITY 用的 X25519 密钥）。
package main

import (
	"bufio"
	"context"
	"crypto/ecdh"
	"crypto/rand"
	"encoding/base64"
	"encoding/binary"
	"errors"
	"flag"
	"fmt"
	"io"
	"log/slog"
	"net"
	"net/http"
	"net/url"
	"os"
	"strconv"
	"strings"
	"time"

	"github.com/enfein/mieru/v3/apis/client"
	apicommon "github.com/enfein/mieru/v3/apis/common"
	"github.com/enfein/mieru/v3/apis/model"
	"github.com/enfein/mieru/v3/pkg/appctl/appctlpb"
	box "github.com/sagernet/sing-box"
	"github.com/sagernet/sing-box/adapter"
	C "github.com/sagernet/sing-box/constant"
	"github.com/sagernet/sing-box/include"
	"github.com/sagernet/sing-box/option"
	M "github.com/sagernet/sing/common/metadata"
	N "github.com/sagernet/sing/common/network"
	"google.golang.org/protobuf/proto"
)

// 等回复的时限（-timeout）：被节点拒绝的 TCP 连接通常马上断开；UDP 包被丢掉时只能等到这个时限
var replyTimeout = 8 * time.Second

type options struct {
	proto     string
	server    string
	password  string
	method    string
	uuid      string
	user      string
	sni       string
	publicKey string
	shortID   string
}

func main() {
	var o options
	flag.StringVar(&o.proto, "proto", "", "协议：shadowsocks、vless、hysteria2、anytls、mieru")
	flag.StringVar(&o.server, "server", "", "节点地址，IP:端口")
	flag.StringVar(&o.password, "password", "", "密码（shadowsocks 是「服务端主密钥:用户密钥」）")
	flag.StringVar(&o.method, "method", "2022-blake3-aes-128-gcm", "shadowsocks 的加密方式")
	flag.StringVar(&o.uuid, "uuid", "", "vless 的 UUID")
	flag.StringVar(&o.user, "user", "", "mieru 的用户名")
	flag.StringVar(&o.sni, "sni", "localhost", "TLS 的 SNI（vless 是伪装目标的域名）")
	flag.StringVar(&o.publicKey, "reality-public-key", "", "REALITY 公钥")
	flag.StringVar(&o.shortID, "short-id", "", "REALITY short ID")
	tcpEcho := flag.String("tcp-echo", "", "经节点连这个回显服务")
	httpURL := flag.String("http", "", "经节点访问这个 http:// 地址")
	udpEcho := flag.String("udp-echo", "", "经节点给这个回显服务发一个 UDP 包")
	dnsServer := flag.String("dns", "", "经节点向这个 DNS 服务器查询")
	name := flag.String("name", "example.com", "DNS 查询的域名")
	echoServer := flag.String("echo-server", "", "自己当 TCP 和 UDP 回显服务，监听这个地址")
	keygen := flag.Bool("reality-keygen", false, "生成一对 REALITY 密钥后退出")
	flag.DurationVar(&replyTimeout, "timeout", replyTimeout, "连接和等回复的时限")
	flag.Parse()
	slog.SetDefault(slog.New(slog.NewTextHandler(os.Stderr, nil)))

	switch {
	case *keygen:
		key, err := ecdh.X25519().GenerateKey(rand.Reader)
		if err != nil {
			fmt.Fprintln(os.Stderr, err)
			os.Exit(1)
		}
		fmt.Printf("REALITY_PRIVATE=%s\nREALITY_PUBLIC=%s\n",
			base64.RawURLEncoding.EncodeToString(key.Bytes()), base64.RawURLEncoding.EncodeToString(key.PublicKey().Bytes()))
		return
	case *echoServer != "":
		slog.Error("回显服务退出", "err", serveEcho(*echoServer))
		os.Exit(1)
	}

	ctx := context.Background()
	d, err := newDialer(ctx, o)
	if err != nil {
		slog.Error("创建客户端", "err", err)
		os.Exit(1)
	}
	defer d.Close()

	failed := false
	run := func(check, target string, f func() (string, error)) {
		start := time.Now()
		detail, err := f()
		elapsed := time.Since(start).Milliseconds()
		if err != nil {
			failed = true
			fmt.Printf("结果 %s %s 失败 %dms %v\n", check, target, elapsed, err)
			return
		}
		fmt.Printf("结果 %s %s 成功 %dms %s\n", check, target, elapsed, detail)
	}
	if *tcpEcho != "" {
		run("tcp-echo", *tcpEcho, func() (string, error) { return d.tcpEcho(ctx, *tcpEcho) })
	}
	if *httpURL != "" {
		run("http", *httpURL, func() (string, error) { return d.fetch(ctx, *httpURL) })
	}
	switch {
	case *dnsServer != "" && *udpEcho != "":
		// 同一个 UDP 会话：先查 DNS（允许的目标），再发往回显服务（检查每个包的目标）
		pc, err := d.listenUDP(ctx, *dnsServer)
		if err != nil {
			run("udp", *dnsServer, func() (string, error) { return "", err })
			break
		}
		run("dns", *dnsServer, func() (string, error) { return dnsQuery(pc, *dnsServer, *name) })
		run("udp-echo", *udpEcho, func() (string, error) { return udpRoundTrip(pc, *udpEcho) })
		pc.Close()
	case *dnsServer != "":
		run("dns", *dnsServer, func() (string, error) {
			pc, err := d.listenUDP(ctx, *dnsServer)
			if err != nil {
				return "", err
			}
			defer pc.Close()
			return dnsQuery(pc, *dnsServer, *name)
		})
	case *udpEcho != "":
		run("udp-echo", *udpEcho, func() (string, error) {
			pc, err := d.listenUDP(ctx, *udpEcho)
			if err != nil {
				return "", err
			}
			defer pc.Close()
			return udpRoundTrip(pc, *udpEcho)
		})
	}
	if failed {
		os.Exit(1)
	}
}

// dialer 经节点建立 TCP 连接和 UDP 会话。
type dialer struct {
	singbox *box.Box
	out     adapter.Outbound
	mieru   client.Client
}

func newDialer(ctx context.Context, o options) (*dialer, error) {
	host, portStr, err := net.SplitHostPort(o.server)
	if err != nil {
		return nil, fmt.Errorf("节点地址: %w", err)
	}
	port, err := strconv.Atoi(portStr)
	if err != nil {
		return nil, fmt.Errorf("节点端口: %w", err)
	}
	if o.proto == "mieru" {
		c := client.NewClient()
		err := c.Store(&client.ClientConfig{Profile: &appctlpb.ClientProfile{
			ProfileName: proto.String("test"),
			User:        &appctlpb.User{Name: proto.String(o.user), Password: proto.String(o.password)},
			Servers: []*appctlpb.ServerEndpoint{{
				IpAddress:    proto.String(host),
				PortBindings: []*appctlpb.PortBinding{{Port: proto.Int32(int32(port)), Protocol: appctlpb.TransportProtocol_TCP.Enum()}},
			}},
		}})
		if err != nil {
			return nil, err
		}
		if err := c.Start(); err != nil {
			return nil, err
		}
		return &dialer{mieru: c}, nil
	}

	outbound, err := outboundOptions(o, option.ServerOptions{Server: host, ServerPort: uint16(port)})
	if err != nil {
		return nil, err
	}
	bctx := box.Context(ctx, include.InboundRegistry(), include.OutboundRegistry(), include.EndpointRegistry(),
		include.DNSTransportRegistry(), include.ServiceRegistry(), include.CertificateProviderRegistry())
	instance, err := box.New(box.Options{Context: bctx, Options: option.Options{
		Log:       &option.LogOptions{Level: "error", DisableColor: true},
		Outbounds: []option.Outbound{outbound},
	}})
	if err != nil {
		return nil, err
	}
	if err := instance.Start(); err != nil {
		instance.Close()
		return nil, err
	}
	out, ok := instance.Outbound().Outbound("proxy")
	if !ok {
		instance.Close()
		return nil, errors.New("找不到出站")
	}
	return &dialer{singbox: instance, out: out}, nil
}

func outboundOptions(o options, server option.ServerOptions) (option.Outbound, error) {
	insecureTLS := option.OutboundTLSOptionsContainer{TLS: &option.OutboundTLSOptions{Enabled: true, ServerName: o.sni, Insecure: true}}
	switch o.proto {
	case "shadowsocks":
		return option.Outbound{Type: C.TypeShadowsocks, Tag: "proxy", Options: &option.ShadowsocksOutboundOptions{
			ServerOptions: server, Method: o.method, Password: o.password,
		}}, nil
	case "vless":
		return option.Outbound{Type: C.TypeVLESS, Tag: "proxy", Options: &option.VLESSOutboundOptions{
			ServerOptions: server, UUID: o.uuid, Flow: "xtls-rprx-vision",
			OutboundTLSOptionsContainer: option.OutboundTLSOptionsContainer{TLS: &option.OutboundTLSOptions{
				Enabled:    true,
				ServerName: o.sni,
				UTLS:       &option.OutboundUTLSOptions{Enabled: true, Fingerprint: "chrome"},
				Reality:    &option.OutboundRealityOptions{Enabled: true, PublicKey: o.publicKey, ShortID: o.shortID},
			}},
		}}, nil
	case "hysteria2":
		tls := insecureTLS
		tls.TLS.ALPN = []string{"h3"}
		return option.Outbound{Type: C.TypeHysteria2, Tag: "proxy", Options: &option.Hysteria2OutboundOptions{
			ServerOptions: server, Password: o.password, OutboundTLSOptionsContainer: tls,
		}}, nil
	case "anytls":
		return option.Outbound{Type: C.TypeAnyTLS, Tag: "proxy", Options: &option.AnyTLSOutboundOptions{
			ServerOptions: server, Password: o.password, OutboundTLSOptionsContainer: insecureTLS,
		}}, nil
	}
	return option.Outbound{}, fmt.Errorf("不认识的协议 %q", o.proto)
}

func (d *dialer) Close() {
	if d.singbox != nil {
		d.singbox.Close()
	}
	if d.mieru != nil {
		d.mieru.Stop()
	}
}

func (d *dialer) dialTCP(ctx context.Context, target string) (net.Conn, error) {
	ctx, cancel := context.WithTimeout(ctx, replyTimeout)
	defer cancel()
	if d.mieru != nil {
		return d.mieru.DialContext(ctx, mieruAddr("tcp", target))
	}
	return d.out.DialContext(ctx, N.NetworkTCP, M.ParseSocksaddr(target))
}

// listenUDP 建一个经节点的 UDP 会话，target 是第一个包的目标。
func (d *dialer) listenUDP(ctx context.Context, target string) (net.PacketConn, error) {
	ctx, cancel := context.WithTimeout(ctx, replyTimeout)
	defer cancel()
	if d.mieru != nil {
		conn, err := d.mieru.DialContext(ctx, mieruAddr("udp", target))
		if err != nil {
			return nil, err
		}
		return apicommon.NewUDPAssociateWrapper(apicommon.NewPacketOverStreamTunnel(conn)), nil
	}
	return d.out.ListenPacket(ctx, M.ParseSocksaddr(target))
}

func mieruAddr(network, target string) model.NetAddrSpec {
	addr := model.NetAddrSpec{Net: network}
	addr.From(M.ParseSocksaddr(target)) // 解析不了时留空，拨号会报错
	addr.Net = network
	return addr
}

// tcpEcho 发一行字，等回显服务原样送回来。
func (d *dialer) tcpEcho(ctx context.Context, target string) (string, error) {
	conn, err := d.dialTCP(ctx, target)
	if err != nil {
		return "", fmt.Errorf("连接: %w", err)
	}
	defer conn.Close()
	conn.SetDeadline(time.Now().Add(replyTimeout))
	if _, err := conn.Write([]byte("ping\n")); err != nil {
		return "", fmt.Errorf("发送: %w", err)
	}
	line, err := bufio.NewReader(conn).ReadString('\n')
	if err != nil {
		return "", fmt.Errorf("接收: %w", err)
	}
	if line != "ping\n" {
		return "", fmt.Errorf("收到的不对：%q", line)
	}
	return "回显正确", nil
}

// fetch 发一个 HTTP GET，返回状态行和响应体字节数。
func (d *dialer) fetch(ctx context.Context, rawURL string) (string, error) {
	u, err := url.Parse(rawURL)
	if err != nil || u.Scheme != "http" {
		return "", fmt.Errorf("只支持 http:// 地址：%q", rawURL)
	}
	target := u.Host
	if u.Port() == "" {
		target = net.JoinHostPort(u.Hostname(), "80")
	}
	conn, err := d.dialTCP(ctx, target)
	if err != nil {
		return "", fmt.Errorf("连接: %w", err)
	}
	defer conn.Close()
	conn.SetDeadline(time.Now().Add(replyTimeout))
	req, err := http.NewRequest(http.MethodGet, rawURL, nil)
	if err != nil {
		return "", err
	}
	req.Close = true
	if err := req.Write(conn); err != nil {
		return "", fmt.Errorf("发请求: %w", err)
	}
	resp, err := http.ReadResponse(bufio.NewReader(conn), req)
	if err != nil {
		return "", fmt.Errorf("读响应: %w", err)
	}
	defer resp.Body.Close()
	n, _ := io.Copy(io.Discard, resp.Body)
	return fmt.Sprintf("%s，%d 字节", resp.Status, n), nil
}

// udpRoundTrip 发一个包，等回显。
func udpRoundTrip(pc net.PacketConn, target string) (string, error) {
	pc.SetDeadline(time.Now().Add(replyTimeout))
	if _, err := pc.WriteTo([]byte("ping"), M.ParseSocksaddr(target)); err != nil {
		return "", fmt.Errorf("发送: %w", err)
	}
	buf := make([]byte, 2048)
	n, from, err := pc.ReadFrom(buf)
	if err != nil {
		return "", fmt.Errorf("等回显: %w", err)
	}
	if string(buf[:n]) != "ping" {
		return "", fmt.Errorf("收到的不对：%q", buf[:n])
	}
	return fmt.Sprintf("来自 %v 的回显", from), nil
}

// dnsQuery 发一个 A 记录查询，返回回答数。
func dnsQuery(pc net.PacketConn, server, name string) (string, error) {
	pc.SetDeadline(time.Now().Add(replyTimeout))
	id := uint16(time.Now().UnixNano())
	msg := make([]byte, 12, 64)
	binary.BigEndian.PutUint16(msg[0:], id)
	binary.BigEndian.PutUint16(msg[2:], 0x0100) // RD
	binary.BigEndian.PutUint16(msg[4:], 1)      // QDCOUNT
	for _, label := range strings.Split(strings.TrimSuffix(name, "."), ".") {
		msg = append(msg, byte(len(label)))
		msg = append(msg, label...)
	}
	msg = append(msg, 0, 0, 1, 0, 1)
	if _, err := pc.WriteTo(msg, M.ParseSocksaddr(server)); err != nil {
		return "", fmt.Errorf("发查询: %w", err)
	}
	buf := make([]byte, 4096)
	n, _, err := pc.ReadFrom(buf)
	if err != nil {
		return "", fmt.Errorf("收回答: %w", err)
	}
	if n < 12 || binary.BigEndian.Uint16(buf) != id {
		return "", fmt.Errorf("回答不对：%d 字节", n)
	}
	return fmt.Sprintf("回答 %d 条", binary.BigEndian.Uint16(buf[6:])), nil
}

// serveEcho 在 address 上开 TCP 和 UDP 回显服务；每收到一个 UDP 包打一行，测试据此确认包有没有被节点放过来。
func serveEcho(address string) error {
	l, err := net.Listen("tcp", address)
	if err != nil {
		return err
	}
	pc, err := net.ListenPacket("udp", address)
	if err != nil {
		return err
	}
	slog.Info("回显服务启动", "listen", address)
	go func() {
		buf := make([]byte, 65535)
		for {
			n, from, err := pc.ReadFrom(buf)
			if err != nil {
				slog.Error("UDP 回显服务退出", "err", err)
				return
			}
			fmt.Printf("收到 UDP 包 %d 字节，来自 %v\n", n, from)
			pc.WriteTo(buf[:n], from)
		}
	}()
	for {
		conn, err := l.Accept()
		if err != nil {
			return err
		}
		fmt.Printf("收到 TCP 连接，来自 %v\n", conn.RemoteAddr())
		go func() {
			defer conn.Close()
			io.Copy(conn, conn)
		}()
	}
}

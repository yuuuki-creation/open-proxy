// opcore 是 open-proxy 的 Agent 代理内核原型，只用于验证，不是正式实现。
// 它内嵌 sing-box，挂上按用户统计的追踪层，并生成配套的客户端配置和流量源，
// 对应 docs/exec-plans/active/2026-09-17-agent-core-prototype.md 里的 V1–V7。
package main

import (
	"context"
	"encoding/json"
	"flag"
	"fmt"
	"log"
	"net/http"
	"os"
	"os/signal"
	"path/filepath"
	"strings"
	"syscall"
	"time"
)

func main() {
	// 子命令 udp-test：通过 SOCKS5 的 UDP ASSOCIATE 打 UDP 回显，验证 UDP 方向的计量
	if len(os.Args) > 1 && os.Args[1] == "udp-test" {
		if err := runUDPTest(os.Args[2:]); err != nil {
			log.Fatalf("UDP 测试失败: %v", err)
		}
		return
	}

	var (
		outDir        = flag.String("out", "./run", "输出目录：凭据、证书、客户端配置")
		serverHost    = flag.String("server-host", "127.0.0.1", "客户端配置里填的服务器地址")
		basePort      = flag.Int("base-port", 20001, "四个入站的起始端口")
		socksBase     = flag.Int("socks-base", 20101, "客户端本地 SOCKS 端口起点")
		controlAddr   = flag.String("control", "127.0.0.1:20090", "控制接口地址（读统计）")
		httpAddr      = flag.String("http", "127.0.0.1:20010", "测试用 HTTP 流量源")
		httpsAddr     = flag.String("https", "127.0.0.1:20011", "测试用 HTTPS 流量源")
		udpAddr       = flag.String("udp-echo", "127.0.0.1:20012", "测试用 UDP 回显服务")
		realityTarget = flag.String("reality-target", "www.cloudflare.com", "REALITY 伪装目标")
		ssMethod      = flag.String("ss-method", "2022-blake3-aes-128-gcm", "Shadowsocks 加密方式")
		certHost      = flag.String("cert-host", "test.local", "自签证书里的名字")
		userCount     = flag.Int("users", 2, "用户数")
		logLevel      = flag.String("log-level", "warn", "sing-box 日志级别")
		useTracker    = flag.Bool("tracker", true, "是否挂统计追踪层（关掉用于对比 splice 行为）")
		ssManaged     = flag.Bool("ss-managed", false, "Shadowsocks 入站声明为 managed，用于试运行时改用户接口")
	)
	flag.Parse()

	if err := run(*outDir, *serverHost, *basePort, *socksBase, *controlAddr, *httpAddr, *httpsAddr, *udpAddr,
		*realityTarget, *ssMethod, *certHost, *userCount, *logLevel, *useTracker, *ssManaged); err != nil {
		log.Fatalf("启动失败: %v", err)
	}
}

// runUDPTest 是 udp-test 子命令：打 UDP 回显并输出收发字节数（JSON）。
func runUDPTest(args []string) error {
	fs := flag.NewFlagSet("udp-test", flag.ExitOnError)
	socksAddr := fs.String("socks", "127.0.0.1:20102", "客户端的本地 SOCKS 地址")
	target := fs.String("target", "127.0.0.1:20012", "UDP 回显服务地址")
	packets := fs.Int("packets", 200, "发包数量")
	size := fs.Int("size", 1200, "每个包的字节数")
	timeout := fs.Duration("timeout", 10*time.Second, "单步超时")
	if err := fs.Parse(args); err != nil {
		return err
	}
	result, err := RunUDPTest(*socksAddr, *target, *packets, *size, *timeout)
	if result != nil {
		data, _ := json.Marshal(result)
		fmt.Println(string(data))
	}
	return err
}

func run(outDir, serverHost string, basePort, socksBase int, controlAddr, httpAddr, httpsAddr, udpAddr,
	realityTarget, ssMethod, certHost string, userCount int, logLevel string, useTracker, ssManaged bool,
) error {
	if err := os.MkdirAll(outDir, 0o755); err != nil {
		return err
	}

	ssKeyLen := 16
	if ssMethod == "2022-blake3-aes-256-gcm" {
		ssKeyLen = 32
	}
	creds, err := NewCreds(userCount, ssKeyLen, outDir, []string{certHost, "127.0.0.1"})
	if err != nil {
		return fmt.Errorf("生成凭据: %w", err)
	}

	cfg := ServerConfig{
		Creds:         creds,
		BasePort:      uint16(basePort),
		RealityTarget: realityTarget,
		SSMethod:      ssMethod,
		CertHost:      certHost,
		LogLevel:      logLevel,
		SSManaged:     ssManaged,
	}

	stopTest, err := StartTestServers(httpAddr, httpsAddr, creds.CertPath, creds.KeyPath)
	if err != nil {
		return fmt.Errorf("启动流量源: %w", err)
	}
	defer stopTest()

	stopUDP, err := StartUDPEcho(udpAddr)
	if err != nil {
		return fmt.Errorf("启动 UDP 回显: %w", err)
	}
	defer stopUDP()

	clients, err := WriteClientConfigs(outDir, cfg, creds.Users, serverHost, socksBase)
	if err != nil {
		return fmt.Errorf("生成客户端配置: %w", err)
	}
	if err = writeJSON(filepath.Join(outDir, "creds.json"), creds); err != nil {
		return err
	}
	if err = writeJSON(filepath.Join(outDir, "clients.json"), clients); err != nil {
		return err
	}

	tracker := NewTracker()
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()

	attached := tracker
	if !useTracker {
		attached = nil
	}
	manager, err := StartServer(ctx, cfg, attached)
	if err != nil {
		return fmt.Errorf("启动 sing-box: %w", err)
	}
	defer manager.Close()

	stopControl := startControl(controlAddr, tracker, manager, controlDeps{
		outDir: outDir, cfg: cfg, serverHost: serverHost, socksBase: socksBase, ssKeyLen: ssKeyLen,
	})
	defer stopControl()

	fmt.Printf("入站已启动：vless=%d hysteria2=%d anytls=%d shadowsocks=%d\n",
		cfg.portFor(0), cfg.portFor(1), cfg.portFor(2), cfg.portFor(3))
	fmt.Printf("流量源：http://%s  https://%s  udp://%s\n", httpAddr, httpsAddr, udpAddr)
	fmt.Printf("控制接口：http://%s/stats\n", controlAddr)
	fmt.Printf("输出目录：%s（客户端配置 %d 份）\n", outDir, len(clients))

	sig := make(chan os.Signal, 1)
	signal.Notify(sig, syscall.SIGINT, syscall.SIGTERM)
	<-sig
	fmt.Println("收到退出信号，停止中")
	return nil
}

type controlDeps struct {
	outDir     string
	cfg        ServerConfig
	serverHost string
	socksBase  int
	ssKeyLen   int
}

// startControl 提供一个只监听本机的接口：读统计、停用和恢复用户、运行时增删用户。
// 正式实现里这些动作来自主控下发的期望状态，这里只是为了验证。
func startControl(addr string, tracker *Tracker, manager *Manager, deps controlDeps) func() {
	mux := http.NewServeMux()

	writeJSONResp := func(w http.ResponseWriter, v any) {
		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(v)
	}
	// tags 参数用来只重建指定入站，便于分协议测量
	tagsOf := func(r *http.Request) []string {
		raw := r.URL.Query().Get("tags")
		if raw == "" {
			return nil
		}
		return strings.Split(raw, ",")
	}
	refreshConfigs := func() {
		clients, err := WriteClientConfigs(deps.outDir, deps.cfg, manager.Users(), deps.serverHost, deps.socksBase)
		if err != nil {
			log.Printf("刷新客户端配置失败: %v", err)
			return
		}
		writeJSON(filepath.Join(deps.outDir, "clients.json"), clients)
	}

	mux.HandleFunc("/users", func(w http.ResponseWriter, r *http.Request) {
		writeJSONResp(w, manager.Users())
	})
	mux.HandleFunc("/users/add", func(w http.ResponseWriter, r *http.Request) {
		user := r.URL.Query().Get("user")
		if user == "" {
			http.Error(w, "缺少 user 参数", http.StatusBadRequest)
			return
		}
		start := time.Now()
		u, err := manager.AddUser(user, deps.ssKeyLen, tagsOf(r))
		if err != nil {
			http.Error(w, err.Error(), http.StatusBadRequest)
			return
		}
		refreshConfigs()
		writeJSONResp(w, map[string]any{"user": u.Name, "rebuild_ms": time.Since(start).Milliseconds()})
	})
	mux.HandleFunc("/users/remove", func(w http.ResponseWriter, r *http.Request) {
		user := r.URL.Query().Get("user")
		if user == "" {
			http.Error(w, "缺少 user 参数", http.StatusBadRequest)
			return
		}
		start := time.Now()
		if err := manager.RemoveUser(user, tagsOf(r)); err != nil {
			http.Error(w, err.Error(), http.StatusBadRequest)
			return
		}
		refreshConfigs()
		writeJSONResp(w, map[string]any{"user": user, "rebuild_ms": time.Since(start).Milliseconds()})
	})
	mux.HandleFunc("/users/ss-update", func(w http.ResponseWriter, r *http.Request) {
		start := time.Now()
		if err := manager.UpdateSSUsers(); err != nil {
			http.Error(w, err.Error(), http.StatusBadRequest)
			return
		}
		writeJSONResp(w, map[string]any{"ok": true, "ms": time.Since(start).Milliseconds()})
	})
	mux.HandleFunc("/stats", func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(tracker.Snapshot())
	})
	mux.HandleFunc("/conns", func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(tracker.ConnCounts())
	})
	mux.HandleFunc("/disable", func(w http.ResponseWriter, r *http.Request) {
		user := r.URL.Query().Get("user")
		if user == "" {
			http.Error(w, "缺少 user 参数", http.StatusBadRequest)
			return
		}
		closed := tracker.DisableUser(user)
		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(map[string]any{"user": user, "closed": closed})
	})
	mux.HandleFunc("/enable", func(w http.ResponseWriter, r *http.Request) {
		user := r.URL.Query().Get("user")
		if user == "" {
			http.Error(w, "缺少 user 参数", http.StatusBadRequest)
			return
		}
		tracker.EnableUser(user)
		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(map[string]any{"user": user, "disabled": false})
	})
	srv := &http.Server{Addr: addr, Handler: mux, ReadHeaderTimeout: 5 * time.Second}
	go func() {
		if err := srv.ListenAndServe(); err != nil && err != http.ErrServerClosed {
			log.Printf("控制接口退出: %v", err)
		}
	}()
	return func() { srv.Close() }
}

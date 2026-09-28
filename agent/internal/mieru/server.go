// Package mieru 运行 Mieru 节点的服务端：每个节点一个 mieru 多路复用器（mux）。
// 收到的连接由 Agent 回 socks5 应答，再带上节点 tag 和用户交给 sing-box 路由，
// 统计、断连、落地出口都和其他协议走同一条路（architecture.md「Agent 内部」）。
//
// 没用 mieru 给第三方集成的 apis/server，而是直接用它底下的 pkg/protocol：
// apis/server 改用户只能停掉实例再建新的，而停实例会关掉所有底层连接，节点上所有人都会断线；
// mux 可以在运行时换用户列表（mieru 自己的服务端 mita 重载配置也是这样做的）。见 ExecPlan 的决策记录。
// 和 core 一样本身不加锁，只在状态管理的 goroutine 里调用。
package mieru

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"errors"
	"fmt"
	"log/slog"
	"net"
	"sync"

	"github.com/enfein/mieru/v3/apis/trafficpattern"
	"github.com/enfein/mieru/v3/pkg/appctl/appctlcommon"
	"github.com/enfein/mieru/v3/pkg/appctl/appctlpb"
	mierucommon "github.com/enfein/mieru/v3/pkg/common"
	mierulog "github.com/enfein/mieru/v3/pkg/log"
	"github.com/enfein/mieru/v3/pkg/protocol"
	"github.com/sagernet/sing-box/adapter"
	"google.golang.org/protobuf/proto"
)

// Type 是 Mieru 节点在 core.InboundConfig 里的类型名，用来和 sing-box 的入站类型区分。
const Type = "mieru"

// User 是一个能用这个节点的用户。
type User struct {
	// 用户 ID 的十进制字符串，和 sing-box 里的用户名一样
	Name     string
	Password string
}

// Options 是一个 Mieru 节点的配置。传输只用 TCP（nodes.md「各协议参数」）。
type Options struct {
	Port  uint16
	Users []User
}

// ErrPasswordChanged 表示有用户的密码和这个实例以前用过的不一样，只换用户列表不够，要重建实例：
// 用旧密码建立的底层连接还能继续开新会话，只有关掉所有底层连接才能让旧密码立即失效。
var ErrPasswordChanged = errors.New("有用户的密码变了，要重建实例才能让旧密码失效")

// Server 是一个 Mieru 节点的服务端实例。
type Server struct {
	tag       string
	router    adapter.ConnectionRouterEx
	ctx       context.Context
	cancel    context.CancelFunc
	mux       *protocol.Mux
	listeners *listenerFactory
	log       *slog.Logger
	closeOnce sync.Once

	// 这个实例用过的每个用户名的密码，用户被移除后也留着：
	// 同一个用户以后换了密码再加回来，旧密码建立的底层连接可能还在，也要重建
	passwords map[string]string
}

// Check 校验配置：用户和端口是否合法。不监听端口。
func Check(opts *Options) error {
	if _, err := buildUsers(opts.Users); err != nil {
		return err
	}
	if _, err := endpoints(opts.Port); err != nil {
		return err
	}
	return nil
}

// Start 按配置监听端口，开始接受连接。连接交给 router 路由，入站 tag 是 tag。
func Start(ctx context.Context, router adapter.ConnectionRouterEx, tag string, opts *Options) (*Server, error) {
	setupLog()
	users, err := buildUsers(opts.Users)
	if err != nil {
		return nil, err
	}
	eps, err := endpoints(opts.Port)
	if err != nil {
		return nil, err
	}
	// 不设流量模式，用 mieru 的默认值（和 mita、apis/server 不配置时一样）
	pattern, err := trafficpattern.NewConfig(nil)
	if err != nil {
		return nil, fmt.Errorf("生成流量模式: %w", err)
	}

	listeners := &listenerFactory{}
	mux := protocol.NewMux(false).
		SetTrafficPattern(pattern).
		SetServerUsers(users).
		SetStreamListenerFactory(listeners).
		SetEndpoints(eps)
	if err := mux.Start(); err != nil {
		// Start 监听失败时自己会关掉 mux；其他原因的失败不会，Close 可以重复调用
		mux.Close()
		listeners.closeAll()
		return nil, fmt.Errorf("监听端口 %d: %w", opts.Port, err)
	}

	sctx, cancel := context.WithCancel(ctx)
	s := &Server{
		tag:       tag,
		router:    router,
		ctx:       sctx,
		cancel:    cancel,
		mux:       mux,
		listeners: listeners,
		log:       slog.With("module", "mieru", "inbound", tag),
		passwords: passwordsOf(opts.Users),
	}
	go s.acceptLoop()
	return s, nil
}

// UpdateUsers 在运行中换掉用户列表，已有的连接不受影响。被移除的用户由追踪层断开和拒绝。
// 有用户的密码和以前用过的不一样时不换，返回 ErrPasswordChanged，由调用方重建实例。
func (s *Server) UpdateUsers(users []User) error {
	for _, u := range users {
		if old, ok := s.passwords[u.Name]; ok && old != u.Password {
			return ErrPasswordChanged
		}
	}
	built, err := buildUsers(users)
	if err != nil {
		return err
	}
	s.mux.SetServerUsers(built)
	for _, u := range users {
		s.passwords[u.Name] = u.Password
	}
	return nil
}

// Close 停掉实例：同步关掉监听和所有底层 TCP 连接，端口立即空出来，这个节点上的客户端立即断开。
// mux 放到后台关：它逐个关会话，每个会话最多等 1 秒把关闭请求发出去，底层连接断了就一定等满
// （mieru v3.38.0 的 Session.closeWithError），会话多时要很久，不能让状态管理和退出流程等它。
func (s *Server) Close() error {
	s.closeOnce.Do(func() {
		s.cancel()
		s.listeners.closeAll()
		go s.mux.Close()
	})
	return nil
}

// acceptLoop 一直接受新连接，每个连接起一个 goroutine 处理，直到实例关闭。
func (s *Server) acceptLoop() {
	for {
		conn, err := s.mux.Accept()
		if err != nil {
			// 监听只在关闭时才会报错（见 listener.go），所以这里通常是实例关了
			if s.ctx.Err() == nil {
				s.log.Error("Mieru 服务端不再接受新连接", "err", err)
			}
			return
		}
		go s.handle(conn)
	}
}

// placeholderName 是用户列表为空时放进去的占位用户：mux 没有用户就启动不了。
// 用户 ID 从 1 开始，0 不是任何用户；密码随机，谁也连不上，万一连上了追踪层也不放行。
const placeholderName = "0"

// buildUsers 把用户列表转成 mux 要的格式，并逐个校验。
func buildUsers(users []User) (map[string]*appctlpb.User, error) {
	result := make(map[string]*appctlpb.User, len(users)+1)
	for _, u := range users {
		if _, dup := result[u.Name]; dup {
			return nil, fmt.Errorf("用户 %s 重复", u.Name)
		}
		pbUser := &appctlpb.User{Name: proto.String(u.Name), Password: proto.String(u.Password)}
		if err := appctlcommon.ValidateServerConfigSingleUser(pbUser); err != nil {
			return nil, fmt.Errorf("用户 %s: %w", u.Name, err)
		}
		result[u.Name] = pbUser
	}
	if len(result) == 0 {
		var b [32]byte
		if _, err := rand.Read(b[:]); err != nil {
			return nil, fmt.Errorf("生成占位用户的密码: %w", err)
		}
		result[placeholderName] = &appctlpb.User{Name: proto.String(placeholderName), Password: proto.String(hex.EncodeToString(b[:]))}
	}
	return result, nil
}

func passwordsOf(users []User) map[string]string {
	m := make(map[string]string, len(users))
	for _, u := range users {
		m[u.Name] = u.Password
	}
	return m
}

// endpoints 生成监听的地址：只监听 IPv4 的 TCP（nodes.md「只支持 IPv4」）。
func endpoints(port uint16) ([]protocol.UnderlayProperties, error) {
	if port == 0 {
		return nil, errors.New("端口不能是 0")
	}
	bindings := []*appctlpb.PortBinding{{
		Port:     proto.Int32(int32(port)),
		Protocol: appctlpb.TransportProtocol_TCP.Enum(),
	}}
	eps, err := appctlcommon.AddrPortToUnderlayProperties(net.IPv4zero.String(), bindings, mierucommon.DefaultMTU)
	if err != nil {
		return nil, fmt.Errorf("端口 %d: %w", port, err)
	}
	return eps, nil
}

var logOnce sync.Once

// setupLog 让 mieru 库的日志不再直接打到标准输出，警告以上的转进 Agent 自己的日志。
func setupLog() {
	logOnce.Do(func() {
		mierulog.SetFormatter(&mierulog.NilFormatter{})
		mierulog.SetLevel("WARN")
		mierulog.SetCallback(func(m mierulog.LogMessage) {
			slog.Warn("mieru 库的日志", "level", m.Level, "msg", m.Message)
		})
	})
}

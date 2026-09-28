// Package conn 负责和主控之间的 WebSocket 连接：发 Hello 认证、收发消息、断线重连。
// 连接流程和消息规则见 main 分支的 docs/design-docs/protocol.md。
package conn

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"net/url"
	"sync"
	"time"

	"github.com/coder/websocket"

	agentv1 "github.com/yuuuki-creation/open-proxy/agent/internal/pb/openproxy/agent/v1"
)

var (
	// ErrServerDeleted 表示主控回复「服务器已删除」。Run 随之返回，由调用方执行卸载。
	ErrServerDeleted = errors.New("服务器已在面板上删除")
	// ErrNotConnected 表示当前没有已认证的连接，消息没有发出去。
	ErrNotConnected = errors.New("没有连上主控")
	// ErrNotSynced 表示和主控版本不一致，只能发稳定消息。
	ErrNotSynced = errors.New("和主控版本不一致，只能发稳定消息")
	// ErrUnsupported 由 Handler.OnRequest 返回，表示不处理这个请求；Client 会回 ErrorReply「不支持」。
	ErrUnsupported = errors.New("不支持这个请求")

	errInvalidToken = errors.New("主控说 Token 无效")
	errDuplicate    = errors.New("主控说这台服务器已经有一个连接在线")
)

const (
	wsPath         = "/api/agent/ws"
	maxMessageSize = 4 << 20 // 单条消息上限 4 MiB，见 protocol.md「传输与外壳」
	dialTimeout    = 15 * time.Second
	helloTimeout   = 15 * time.Second
	writeTimeout   = 10 * time.Second
	pingInterval   = 30 * time.Second
	pingTimeout    = 10 * time.Second
)

// Config 是建立连接需要的固定信息。
type Config struct {
	// 主控地址，https://主控域名，不带路径；本机测试时可以是 http://127.0.0.1:端口（由配置校验保证）。
	MasterURL string
	Token     string
	// Agent 版本，和主控一致才同步期望状态。
	Version string
	Arch    agentv1.Arch
	// 本次进程启动时随机生成，Hello 和流量上报都带它。
	InstanceID uint64
}

// Handler 由使用 conn 的模块实现，处理主控发来的业务消息。
type Handler interface {
	// HelloState 返回 Hello 里由其他模块提供的字段：本地已应用的期望状态版本、
	// 上次升级失败回滚时的目标版本。每次连接前调用。
	HelloState() (stateVersion uint64, rolledBackFrom string)
	// OnConnected 在认证成功后、处理主控的任何后续消息之前调用。
	// sync 为 false 表示和主控版本不一致，只能收发稳定消息。
	// 这时收消息的 goroutine 在等它返回：可以用 Send 发推送，但不能等主控的回复，否则会一直等到超时。
	OnConnected(sync bool)
	// OnDisconnected 在已认证的连接断开后调用。
	OnDisconnected()
	// OnPush 处理主控的推送（期望状态）。在收消息的 goroutine 里调用，
	// 耗时的工作要转交出去，不要阻塞。
	OnPush(msg *agentv1.MasterMessage)
	// OnRequest 处理主控的请求，返回回复的消息体（只填 Body，ID 由 Client 填）。
	// 每个请求在单独的 goroutine 里调用，可以耗时；连接断开时 ctx 被取消。
	// 返回 ErrUnsupported 时回「不支持」，返回其他错误时回「处理失败」。
	OnRequest(ctx context.Context, msg *agentv1.MasterMessage) (*agentv1.AgentMessage, error)
	// AfterReply 在请求的回复发出之后调用（和 OnRequest 在同一个 goroutine 里），
	// 用来做必须等回复发出去才能做的事，例如升级后退出。sendErr 是发回复的错误，发成功时为 nil。
	AfterReply(msg *agentv1.MasterMessage, sendErr error)
}

// Client 维持和主控的连接，断开后按退避时间自动重连。
type Client struct {
	cfg     Config
	wsURL   string
	handler Handler
	log     *slog.Logger

	mu      sync.Mutex
	current *session // 已认证的连接；没有时为 nil
}

// New 创建 Client，不会立即连接；调用 Run 开始。
func New(cfg Config, handler Handler) (*Client, error) {
	u, err := url.Parse(cfg.MasterURL)
	if err != nil || u.Host == "" {
		return nil, fmt.Errorf("主控地址不对: %q", cfg.MasterURL)
	}
	scheme := "wss"
	if u.Scheme == "http" {
		scheme = "ws"
	}
	return &Client{
		cfg:     cfg,
		wsURL:   scheme + "://" + u.Host + wsPath,
		handler: handler,
		log:     slog.With("module", "conn"),
	}, nil
}

// Run 连接主控并保持连接，断开后按退避时间重连，直到 ctx 取消。
// 主控回复「服务器已删除」时返回 ErrServerDeleted；其他情况只在 ctx 取消时返回 nil。
func (c *Client) Run(ctx context.Context) error {
	var b backoff
	for {
		res, err := c.runSession(ctx)
		if ctx.Err() != nil {
			return nil
		}
		if errors.Is(err, ErrServerDeleted) {
			return err
		}
		// 连接稳定用过一段时间才重置退避，避免「连上就断」时频繁重连
		if res.authenticated && res.duration >= stableDuration {
			b.reset()
		}
		wait := b.next()
		if errors.Is(err, errInvalidToken) {
			wait = invalidTokenWait
		}
		wait = jitter(wait)
		c.log.Warn("和主控的连接断开，稍后重连", "err", err, "wait", wait.Round(time.Second))

		timer := time.NewTimer(wait)
		select {
		case <-ctx.Done():
			timer.Stop()
			return nil
		case <-timer.C:
		}
	}
}

// Send 给主控发一条推送，消息 ID 由 Client 填写。
// 没有连接时返回 ErrNotConnected；版本不一致时只允许发稳定消息，否则返回 ErrNotSynced。
func (c *Client) Send(ctx context.Context, msg *agentv1.AgentMessage) error {
	c.mu.Lock()
	s := c.current
	c.mu.Unlock()
	if s == nil {
		return ErrNotConnected
	}
	if !s.sync && !isStableAgentMessage(msg) {
		return ErrNotSynced
	}
	return s.send(ctx, msg)
}

func (c *Client) setCurrent(s *session) {
	c.mu.Lock()
	c.current = s
	c.mu.Unlock()
}

type sessionResult struct {
	authenticated bool
	duration      time.Duration
}

// runSession 建立一次连接并一直处理到断开，返回断开的原因。
func (c *Client) runSession(ctx context.Context) (sessionResult, error) {
	var res sessionResult

	dialCtx, cancelDial := context.WithTimeout(ctx, dialTimeout)
	ws, _, err := websocket.Dial(dialCtx, c.wsURL, &websocket.DialOptions{
		CompressionMode: websocket.CompressionDisabled,
	})
	cancelDial()
	if err != nil {
		return res, fmt.Errorf("连接 %s: %w", c.wsURL, err)
	}
	defer ws.CloseNow()
	ws.SetReadLimit(maxMessageSize)

	sctx, stop := context.WithCancel(ctx)
	defer stop()
	s := newSession(sctx, ws, c.handler, c.log)
	readDone := make(chan error, 1)
	go func() { readDone <- s.readLoop() }()

	result, err := c.hello(sctx, s)
	if err != nil {
		return res, err
	}
	switch result.GetStatus() {
	case agentv1.HelloResult_STATUS_OK:
	case agentv1.HelloResult_STATUS_INVALID_TOKEN:
		return res, errInvalidToken
	case agentv1.HelloResult_STATUS_DUPLICATE_CONNECTION:
		return res, errDuplicate
	case agentv1.HelloResult_STATUS_SERVER_DELETED:
		return res, ErrServerDeleted
	default:
		return res, fmt.Errorf("主控回复了不认识的认证结果 %v", result.GetStatus())
	}

	// 认证成功：先登记连接、通知 Handler，再放行收消息的 goroutine 处理后续消息，
	// 这样紧跟在 HelloResult 后面的期望状态不会在准备好之前被处理
	start := time.Now()
	res.authenticated = true
	s.sync = result.GetSync()
	c.setCurrent(s)
	c.handler.OnConnected(s.sync)
	close(s.ready)
	c.log.Info("已连上主控", "master_version", result.GetMasterVersion(), "sync", s.sync)

	go s.pingLoop()
	err = <-readDone

	c.setCurrent(nil)
	stop()
	s.failPending()
	c.handler.OnDisconnected()
	res.duration = time.Since(start)
	return res, err
}

// hello 发 Hello 并等主控回复 HelloResult。
func (c *Client) hello(ctx context.Context, s *session) (*agentv1.HelloResult, error) {
	stateVersion, rolledBackFrom := c.handler.HelloState()
	msg := &agentv1.AgentMessage{Body: &agentv1.AgentMessage_Hello{Hello: &agentv1.Hello{
		Token:          c.cfg.Token,
		AgentVersion:   c.cfg.Version,
		Arch:           c.cfg.Arch,
		InstanceId:     c.cfg.InstanceID,
		StateVersion:   stateVersion,
		RolledBackFrom: rolledBackFrom,
	}}}

	ctx, cancel := context.WithTimeout(ctx, helloTimeout)
	defer cancel()
	reply, err := s.request(ctx, msg)
	if err != nil {
		return nil, fmt.Errorf("等主控回复 Hello: %w", err)
	}
	result := reply.GetHelloResult()
	if result == nil {
		return nil, errors.New("主控对 Hello 的回复不是 HelloResult")
	}
	return result, nil
}

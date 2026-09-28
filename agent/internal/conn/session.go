package conn

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"sync"
	"sync/atomic"
	"time"

	"github.com/coder/websocket"
	"google.golang.org/protobuf/proto"

	agentv1 "github.com/yuuuki-creation/open-proxy/agent/internal/pb/openproxy/agent/v1"
)

// session 是一次 WebSocket 连接：消息 ID、等待回复的请求表、收消息和心跳。
// 连接断开后整个 session 作废，重连时新建。
type session struct {
	ctx     context.Context // 连接断开时取消
	ws      *websocket.Conn
	handler Handler
	log     *slog.Logger

	// 认证成功后由 runSession 设置，然后关闭 ready；之后只读
	sync  bool
	ready chan struct{}

	nextID atomic.Uint64

	mu      sync.Mutex
	pending map[uint64]chan *agentv1.MasterMessage // 请求 ID -> 等回复的通道
	closed  bool
}

func newSession(ctx context.Context, ws *websocket.Conn, handler Handler, log *slog.Logger) *session {
	return &session{
		ctx:     ctx,
		ws:      ws,
		handler: handler,
		log:     log,
		ready:   make(chan struct{}),
		pending: make(map[uint64]chan *agentv1.MasterMessage),
	}
}

// send 发一条不需要回复的消息，填上新的消息 ID。
func (s *session) send(ctx context.Context, msg *agentv1.AgentMessage) error {
	msg.Id = s.nextID.Add(1)
	return s.write(ctx, msg)
}

// request 发一条请求并等主控回复。主控回 ErrorReply 时返回错误。
func (s *session) request(ctx context.Context, msg *agentv1.AgentMessage) (*agentv1.MasterMessage, error) {
	id := s.nextID.Add(1)
	msg.Id = id
	ch := make(chan *agentv1.MasterMessage, 1)

	s.mu.Lock()
	if s.closed {
		s.mu.Unlock()
		return nil, ErrNotConnected
	}
	s.pending[id] = ch
	s.mu.Unlock()
	defer func() {
		s.mu.Lock()
		delete(s.pending, id)
		s.mu.Unlock()
	}()

	if err := s.write(ctx, msg); err != nil {
		return nil, err
	}
	select {
	case reply, ok := <-ch:
		if !ok {
			return nil, ErrNotConnected
		}
		if e := reply.GetErrorReply(); e != nil {
			return nil, fmt.Errorf("主控回复错误（%v）: %s", e.GetCode(), e.GetMessage())
		}
		return reply, nil
	case <-ctx.Done():
		return nil, ctx.Err()
	}
}

// write 编码并发出一条消息，超时 10 秒。
// coder/websocket 的 Write 可以并发调用，库内部会把写操作串行化。
func (s *session) write(ctx context.Context, msg *agentv1.AgentMessage) error {
	data, err := proto.Marshal(msg)
	if err != nil {
		return fmt.Errorf("编码消息: %w", err)
	}
	ctx, cancel := context.WithTimeout(ctx, writeTimeout)
	defer cancel()
	if err := s.ws.Write(ctx, websocket.MessageBinary, data); err != nil {
		return fmt.Errorf("发送消息: %w", err)
	}
	return nil
}

// readLoop 一直收消息直到连接断开，返回断开的原因。
func (s *session) readLoop() error {
	for {
		typ, data, err := s.ws.Read(s.ctx)
		if err != nil {
			return fmt.Errorf("收消息: %w", err)
		}
		if typ != websocket.MessageBinary {
			s.log.Warn("收到非二进制消息，忽略")
			continue
		}
		msg := &agentv1.MasterMessage{}
		if err := proto.Unmarshal(data, msg); err != nil {
			s.log.Warn("消息解码失败，忽略", "err", err)
			continue
		}
		s.dispatch(msg)
	}
}

// dispatch 按消息类型分发：回复交给等待的请求，请求起 goroutine 处理，推送交给 Handler。
func (s *session) dispatch(msg *agentv1.MasterMessage) {
	if msg.GetReplyTo() != 0 {
		s.deliverReply(msg)
		if msg.GetHelloResult() != nil {
			// 等 runSession 处理完认证结果，再处理后面的消息
			s.waitReady()
		}
		return
	}
	if !s.waitReady() {
		return
	}
	switch {
	case msg.GetBody() == nil:
		// 不认识的消息（通常是主控版本更新），分不清是请求还是推送，只能忽略
		s.log.Debug("收到不认识的消息，忽略", "id", msg.GetId())
	case isMasterRequest(msg):
		go s.handleRequest(msg)
	case !s.sync && !isStableMasterMessage(msg):
		s.log.Debug("版本不一致，忽略非稳定的推送", "id", msg.GetId())
	default:
		s.handler.OnPush(msg)
	}
}

// waitReady 等认证完成；连接先断开时返回 false。
func (s *session) waitReady() bool {
	select {
	case <-s.ready:
		return true
	case <-s.ctx.Done():
		return false
	}
}

func (s *session) deliverReply(msg *agentv1.MasterMessage) {
	s.mu.Lock()
	ch, ok := s.pending[msg.GetReplyTo()]
	delete(s.pending, msg.GetReplyTo())
	s.mu.Unlock()
	if !ok {
		s.log.Debug("回复对应的请求已经超时，丢弃", "reply_to", msg.GetReplyTo())
		return
	}
	ch <- msg // 通道有 1 个缓冲，每个请求只收一次回复，不会阻塞
}

// handleRequest 处理主控的一个请求并回复。
func (s *session) handleRequest(msg *agentv1.MasterMessage) {
	var reply *agentv1.AgentMessage
	var err error
	if !s.sync && !isStableMasterMessage(msg) {
		err = fmt.Errorf("%w：和主控版本不一致，只处理稳定消息", ErrUnsupported)
	} else {
		reply, err = s.handler.OnRequest(s.ctx, msg)
		if err == nil && reply == nil {
			err = errors.New("处理完没有回复内容")
		}
	}
	if err != nil {
		code := agentv1.ErrorReply_CODE_FAILED
		if errors.Is(err, ErrUnsupported) {
			code = agentv1.ErrorReply_CODE_UNSUPPORTED
		}
		reply = &agentv1.AgentMessage{Body: &agentv1.AgentMessage_ErrorReply{
			ErrorReply: &agentv1.ErrorReply{Code: code, Message: err.Error()},
		}}
	}
	reply.ReplyTo = msg.GetId()
	sendErr := s.send(s.ctx, reply)
	if sendErr != nil {
		s.log.Warn("回复主控失败", "request_id", msg.GetId(), "err", sendErr)
	}
	s.handler.AfterReply(msg, sendErr)
}

// pingLoop 定时给主控发 ping；没有回应就断开，让 Run 重连。
// 主控那边也会发 ping，库会自动回 pong；这里是为了发现主控那端已经不在了的半开连接。
func (s *session) pingLoop() {
	ticker := time.NewTicker(pingInterval)
	defer ticker.Stop()
	for {
		select {
		case <-s.ctx.Done():
			return
		case <-ticker.C:
			ctx, cancel := context.WithTimeout(s.ctx, pingTimeout)
			err := s.ws.Ping(ctx)
			cancel()
			if err != nil {
				if s.ctx.Err() == nil {
					s.log.Warn("心跳没有回应，断开重连", "err", err)
				}
				s.ws.CloseNow()
				return
			}
		}
	}
}

// failPending 让所有还在等回复的请求立即失败。连接断开后调用。
func (s *session) failPending() {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.closed = true
	for id, ch := range s.pending {
		close(ch)
		delete(s.pending, id)
	}
}

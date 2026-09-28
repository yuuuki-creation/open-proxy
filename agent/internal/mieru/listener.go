package mieru

import (
	"context"
	"errors"
	"log/slog"
	"net"
	"sync"
	"sync/atomic"
	"time"
)

// listenerFactory 给 mux 建 TCP 监听，代替 mieru 默认的做法：
//   - 不设 SO_REUSEPORT（mieru 默认会设）：否则端口已被别的程序这样占着时照样能监听成功，新连接被内核分到两边
//   - 只监听 IPv4：mieru 把 0.0.0.0 按 "tcp" 监听，Go 会建成同时收 IPv6 的套接字；sing-box 的入站都只收 IPv4
//   - 记下建好的监听和接受的 TCP 连接，关实例时同步关掉：端口立即空出来，客户端立即断开。
//     mux 自己是在另一个 goroutine 里异步关监听的，端口不变的重建紧接着监听同一个端口会失败；
//     它关会话又很慢（见 Server.Close），不能等它来断开客户端
//   - Accept 遇到临时错误（例如文件描述符用完）等一会儿再试：mieru 的接收循环遇到任何错误就退出，
//     这个节点之后再也不接新连接
type listenerFactory struct {
	mu        sync.Mutex
	listeners []*listener
	conns     map[*trackedConn]struct{}
	closed    bool
}

func (f *listenerFactory) Listen(ctx context.Context, network, address string) (net.Listener, error) {
	if network == "tcp" {
		network = "tcp4"
	}
	var lc net.ListenConfig
	l, err := lc.Listen(ctx, network, address)
	if err != nil {
		return nil, err
	}
	wrapped := &listener{Listener: l, factory: f}
	f.mu.Lock()
	defer f.mu.Unlock()
	if f.closed {
		l.Close()
		return nil, net.ErrClosed
	}
	f.listeners = append(f.listeners, wrapped)
	return wrapped, nil
}

// track 登记一个接受的连接；已经关闭时直接关掉它，返回 false。
func (f *listenerFactory) track(c *trackedConn) bool {
	f.mu.Lock()
	defer f.mu.Unlock()
	if f.closed {
		c.Conn.Close()
		return false
	}
	if f.conns == nil {
		f.conns = make(map[*trackedConn]struct{})
	}
	f.conns[c] = struct{}{}
	return true
}

func (f *listenerFactory) untrack(c *trackedConn) {
	f.mu.Lock()
	delete(f.conns, c)
	f.mu.Unlock()
}

// closeAll 关掉所有监听和接受的连接，返回时端口已经释放。之后不能再建新的。
func (f *listenerFactory) closeAll() {
	f.mu.Lock()
	f.closed = true
	listeners, conns := f.listeners, f.conns
	f.listeners, f.conns = nil, nil
	f.mu.Unlock()
	for _, l := range listeners {
		l.Close()
	}
	for c := range conns {
		c.Conn.Close()
	}
}

type listener struct {
	net.Listener
	factory *listenerFactory
	closed  atomic.Bool
}

// Accept 只在监听关闭后才返回错误，其他错误等一会儿重试（等待时间 5 毫秒起翻倍，最长 1 秒）。
func (l *listener) Accept() (net.Conn, error) {
	var wait time.Duration
	for {
		conn, err := l.Listener.Accept()
		if err == nil {
			tracked := &trackedConn{Conn: conn, factory: l.factory}
			if !l.factory.track(tracked) {
				return nil, net.ErrClosed
			}
			return tracked, nil
		}
		if l.closed.Load() || errors.Is(err, net.ErrClosed) {
			return nil, err
		}
		wait = min(max(2*wait, 5*time.Millisecond), time.Second)
		slog.Warn("Mieru 监听接受连接出错，稍后重试", "addr", l.Addr(), "err", err, "wait", wait)
		time.Sleep(wait)
	}
}

func (l *listener) Close() error {
	l.closed.Store(true)
	return l.Listener.Close()
}

// trackedConn 是接受的 TCP 连接（mieru 的一条底层连接），关闭时注销登记。
type trackedConn struct {
	net.Conn
	factory *listenerFactory
	once    sync.Once
}

func (c *trackedConn) Close() error {
	c.once.Do(func() { c.factory.untrack(c) })
	return c.Conn.Close()
}

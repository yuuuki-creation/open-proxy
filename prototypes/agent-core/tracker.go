package main

import (
	"context"
	"net"
	"sort"
	"sync"
	"sync/atomic"
	"time"

	"github.com/sagernet/sing-box/adapter"
	tun "github.com/sagernet/sing-tun"
	"github.com/sagernet/sing/common/buf"
	"github.com/sagernet/sing/common/bufio"
	M "github.com/sagernet/sing/common/metadata"
	N "github.com/sagernet/sing/common/network"
)

// statKey 是统计的粒度：哪个入站上的哪个用户。
type statKey struct {
	Inbound string
	User    string
}

type counters struct {
	up   atomic.Int64 // 客户端发上来的字节数
	down atomic.Int64 // 发回客户端的字节数
}

// Tracker 实现 sing-box 的 adapter.ConnectionTracker。
// sing-box 选好出站、拨号之前会调用它，参数里带入站 tag 和用户名，
// 返回的连接会替换原连接。这里做三件事：
//  1. 按「入站 × 用户」累计流量（用 sing 自带的计数连接）
//  2. 登记每条连接，停用用户时能主动断开
//  3. 拒绝已停用用户的新连接
type Tracker struct {
	mu       sync.RWMutex
	data     map[statKey]*counters
	conns    map[string]map[uint64]closerFunc // 用户 -> 连接 ID -> 关闭函数
	disabled map[string]bool
	nextID   atomic.Uint64
}

type closerFunc func() error

func NewTracker() *Tracker {
	return &Tracker{
		data:     make(map[statKey]*counters),
		conns:    make(map[string]map[uint64]closerFunc),
		disabled: make(map[string]bool),
	}
}

func (t *Tracker) counterFor(key statKey) *counters {
	t.mu.RLock()
	c, ok := t.data[key]
	t.mu.RUnlock()
	if ok {
		return c
	}
	t.mu.Lock()
	defer t.mu.Unlock()
	if c, ok = t.data[key]; ok {
		return c
	}
	c = &counters{}
	t.data[key] = c
	return c
}

func (t *Tracker) isDisabled(user string) bool {
	t.mu.RLock()
	defer t.mu.RUnlock()
	return t.disabled[user]
}

func (t *Tracker) register(user string, close closerFunc) uint64 {
	id := t.nextID.Add(1)
	t.mu.Lock()
	if t.conns[user] == nil {
		t.conns[user] = make(map[uint64]closerFunc)
	}
	t.conns[user][id] = close
	t.mu.Unlock()
	return id
}

func (t *Tracker) deregister(user string, id uint64) {
	t.mu.Lock()
	if m := t.conns[user]; m != nil {
		delete(m, id)
	}
	t.mu.Unlock()
}

func (t *Tracker) RoutedConnection(ctx context.Context, conn net.Conn, metadata adapter.InboundContext, matchedRule adapter.Rule, matchOutbound adapter.Outbound) net.Conn {
	user := metadata.User
	if user == "" {
		return conn
	}
	if t.isDisabled(user) {
		// 已停用：直接断开，新连接进不来
		conn.Close()
		return conn
	}
	c := t.counterFor(statKey{Inbound: metadata.Inbound, User: user})
	counted := bufio.NewInt64CounterConn(conn, []*atomic.Int64{&c.up}, []*atomic.Int64{&c.down})
	tracked := &trackedConn{Conn: counted, tracker: t, user: user}
	tracked.id = t.register(user, counted.Close)
	return tracked
}

func (t *Tracker) RoutedPacketConnection(ctx context.Context, conn N.PacketConn, metadata adapter.InboundContext, matchedRule adapter.Rule, matchOutbound adapter.Outbound) N.PacketConn {
	user := metadata.User
	if user == "" {
		return conn
	}
	if t.isDisabled(user) {
		conn.Close()
		return conn
	}
	c := t.counterFor(statKey{Inbound: metadata.Inbound, User: user})
	counted := bufio.NewInt64CounterPacketConn(conn, []*atomic.Int64{&c.up}, nil, []*atomic.Int64{&c.down}, nil)
	tracked := &trackedPacketConn{PacketConn: counted, tracker: t, user: user}
	tracked.id = t.register(user, counted.Close)
	return tracked
}

// RoutedFlow 只用于 TUN 的流量追踪，原型里不需要。
func (t *Tracker) RoutedFlow(ctx context.Context, metadata adapter.InboundContext, matchedRule adapter.Rule, matchOutbound adapter.Outbound) tun.FlowTracker {
	return nil
}

// trackedConn 在计数连接外面再包一层，只为在连接关闭时销号。
// 实现 sing 的「可解包」接口，让上层该怎么优化还怎么优化。
type trackedConn struct {
	net.Conn
	tracker *Tracker
	user    string
	id      uint64
	once    sync.Once
}

func (c *trackedConn) Close() error {
	c.once.Do(func() { c.tracker.deregister(c.user, c.id) })
	return c.Conn.Close()
}

func (c *trackedConn) Upstream() any           { return c.Conn }
func (c *trackedConn) UpstreamReader() any     { return c.Conn }
func (c *trackedConn) UpstreamWriter() any     { return c.Conn }
func (c *trackedConn) ReaderReplaceable() bool { return true }
func (c *trackedConn) WriterReplaceable() bool { return true }

type trackedPacketConn struct {
	N.PacketConn
	tracker *Tracker
	user    string
	id      uint64
	once    sync.Once
}

func (c *trackedPacketConn) Close() error {
	c.once.Do(func() { c.tracker.deregister(c.user, c.id) })
	return c.PacketConn.Close()
}

func (c *trackedPacketConn) Upstream() any { return c.PacketConn }

func (c *trackedPacketConn) ReadPacket(buffer *buf.Buffer) (M.Socksaddr, error) {
	return c.PacketConn.ReadPacket(buffer)
}

func (c *trackedPacketConn) WritePacket(buffer *buf.Buffer, destination M.Socksaddr) error {
	return c.PacketConn.WritePacket(buffer, destination)
}

func (c *trackedPacketConn) SetDeadline(t time.Time) error { return c.PacketConn.SetDeadline(t) }
func (c *trackedPacketConn) SetReadDeadline(t time.Time) error {
	return c.PacketConn.SetReadDeadline(t)
}
func (c *trackedPacketConn) SetWriteDeadline(t time.Time) error {
	return c.PacketConn.SetWriteDeadline(t)
}

// DisableUser 停用一个用户：断开他所有存量连接，并拒绝新连接。
// 返回断开的连接数。
func (t *Tracker) DisableUser(user string) int {
	t.mu.Lock()
	t.disabled[user] = true
	closers := make([]closerFunc, 0, len(t.conns[user]))
	for _, c := range t.conns[user] {
		closers = append(closers, c)
	}
	t.mu.Unlock()
	for _, c := range closers {
		c()
	}
	return len(closers)
}

func (t *Tracker) EnableUser(user string) {
	t.mu.Lock()
	delete(t.disabled, user)
	t.mu.Unlock()
}

// StatRow 是对外输出的一行统计。
type StatRow struct {
	Inbound  string `json:"inbound"`
	User     string `json:"user"`
	Uplink   int64  `json:"uplink"`
	Downlink int64  `json:"downlink"`
}

// Snapshot 返回当前累计值。累计值不清零，由主控算增量。
func (t *Tracker) Snapshot() []StatRow {
	t.mu.RLock()
	rows := make([]StatRow, 0, len(t.data))
	for key, c := range t.data {
		rows = append(rows, StatRow{
			Inbound:  key.Inbound,
			User:     key.User,
			Uplink:   c.up.Load(),
			Downlink: c.down.Load(),
		})
	}
	t.mu.RUnlock()
	sort.Slice(rows, func(i, j int) bool {
		if rows[i].Inbound != rows[j].Inbound {
			return rows[i].Inbound < rows[j].Inbound
		}
		return rows[i].User < rows[j].User
	})
	return rows
}

// ConnCounts 返回每个用户当前登记在案的连接数，以及是否被停用。
func (t *Tracker) ConnCounts() map[string]map[string]any {
	t.mu.RLock()
	defer t.mu.RUnlock()
	out := make(map[string]map[string]any, len(t.conns))
	for user, m := range t.conns {
		out[user] = map[string]any{"conns": len(m), "disabled": t.disabled[user]}
	}
	for user := range t.disabled {
		if _, ok := out[user]; !ok {
			out[user] = map[string]any{"conns": 0, "disabled": true}
		}
	}
	return out
}

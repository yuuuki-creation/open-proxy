// Package tracker 是挂在 sing-box 上的连接追踪层：按「入站 × 用户」计流量、
// 只放行节点放行名单里的用户、名单变化时立即断开被移除的用户。
// 设计见 main 分支 architecture.md「按用户统计」「停用用户时断开存量连接」。
package tracker

import (
	"context"
	"net"
	"slices"
	"strings"
	"sync"
	"sync/atomic"

	"github.com/sagernet/sing-box/adapter"
	tun "github.com/sagernet/sing-tun"
	"github.com/sagernet/sing/common/bufio"
	N "github.com/sagernet/sing/common/network"
)

// NodeTagPrefix 是节点入站 tag 的前缀（node-<节点 ID>）。只有这类入站受放行名单约束，
// 其他入站（自建落地的 SOCKS5）直接放过。
const NodeTagPrefix = "node-"

// Key 是计数和登记连接的粒度：哪个入站上的哪个用户。
type Key struct {
	Inbound string
	User    string
}

// Stat 是一行累计流量。
type Stat struct {
	Key
	// 客户端发上来的字节数
	Uplink int64
	// 发回客户端的字节数
	Downlink int64
}

type counter struct {
	up   atomic.Int64
	down atomic.Int64
}

// Tracker 实现 adapter.ConnectionTracker。sing-box 选好出站、拨号之前调用它，
// 返回的连接会替换原连接。计数放在这里而不是 sing-box 里，所以重建入站不会归零。
type Tracker struct {
	mu       sync.Mutex
	allowed  map[string]map[string]struct{} // 入站 tag -> 放行的用户
	conns    map[Key]map[uint64]func()      // 入站和用户 -> 连接 ID -> 关闭函数
	counters map[Key]*counter
	nextID   uint64
}

var _ adapter.ConnectionTracker = (*Tracker)(nil)

func New() *Tracker {
	return &Tracker{
		allowed:  make(map[string]map[string]struct{}),
		conns:    make(map[Key]map[uint64]func()),
		counters: make(map[Key]*counter),
	}
}

// SetAllowed 设置一个节点入站的放行名单，并立即断开不在名单里的存量连接。
// users 为 nil 表示这个节点已删除：放行名单清空，连接全部断开。返回断开的连接数。
func (t *Tracker) SetAllowed(inbound string, users []string) int {
	var closers []func()
	t.mu.Lock()
	if users == nil {
		delete(t.allowed, inbound)
	} else {
		set := make(map[string]struct{}, len(users))
		for _, u := range users {
			set[u] = struct{}{}
		}
		t.allowed[inbound] = set
	}
	for key, conns := range t.conns {
		if key.Inbound != inbound || t.isAllowedLocked(key) {
			continue
		}
		for _, closeConn := range conns {
			closers = append(closers, closeConn)
		}
		delete(t.conns, key)
	}
	t.mu.Unlock()

	// 在锁外关闭：关闭连接时 sing-box 会回调 trackedConn.Close，那里要拿锁
	for _, closeConn := range closers {
		closeConn()
	}
	return len(closers)
}

// Allowed 判断这个入站上的这个用户现在能不能用。分发出站拨号前也会查一次，
// 被拒绝的连接就不必再去拨目标地址。
func (t *Tracker) Allowed(inbound, user string) bool {
	t.mu.Lock()
	defer t.mu.Unlock()
	return t.isAllowedLocked(Key{Inbound: inbound, User: user})
}

func (t *Tracker) isAllowedLocked(key Key) bool {
	if !strings.HasPrefix(key.Inbound, NodeTagPrefix) {
		return true
	}
	users, ok := t.allowed[key.Inbound]
	if !ok {
		return false
	}
	_, ok = users[key.User]
	return ok
}

// Snapshot 返回全部累计流量，按入站和用户排序。累计值不清零，由主控算增量。
func (t *Tracker) Snapshot() []Stat {
	t.mu.Lock()
	stats := make([]Stat, 0, len(t.counters))
	for key, c := range t.counters {
		stats = append(stats, Stat{Key: key, Uplink: c.up.Load(), Downlink: c.down.Load()})
	}
	t.mu.Unlock()
	slices.SortFunc(stats, func(a, b Stat) int {
		if c := strings.Compare(a.Inbound, b.Inbound); c != 0 {
			return c
		}
		return strings.Compare(a.User, b.User)
	})
	return stats
}

// register 检查是否放行，放行就取计数器并登记连接，返回连接 ID。
// 检查和登记在同一把锁里：否则恰好在停用那一刻建立的连接会漏断（原型里的问题）。
func (t *Tracker) register(key Key, closeConn func()) (*counter, uint64, bool) {
	t.mu.Lock()
	defer t.mu.Unlock()
	if !t.isAllowedLocked(key) {
		return nil, 0, false
	}
	c, ok := t.counters[key]
	if !ok {
		c = &counter{}
		t.counters[key] = c
	}
	t.nextID++
	id := t.nextID
	if t.conns[key] == nil {
		t.conns[key] = make(map[uint64]func())
	}
	t.conns[key][id] = closeConn
	return c, id, true
}

func (t *Tracker) deregister(key Key, id uint64) {
	t.mu.Lock()
	defer t.mu.Unlock()
	if conns := t.conns[key]; conns != nil {
		delete(conns, id)
		if len(conns) == 0 {
			delete(t.conns, key)
		}
	}
}

func (t *Tracker) RoutedConnection(ctx context.Context, conn net.Conn, metadata adapter.InboundContext, matchedRule adapter.Rule, matchOutbound adapter.Outbound) net.Conn {
	if !strings.HasPrefix(metadata.Inbound, NodeTagPrefix) {
		return conn
	}
	key := Key{Inbound: metadata.Inbound, User: metadata.User}
	tracked := &trackedConn{tracker: t, key: key}
	c, id, ok := t.register(key, func() { conn.Close() })
	if !ok {
		// 不在放行名单里：直接断开，新连接进不来
		conn.Close()
		return conn
	}
	tracked.id = id
	tracked.Conn = bufio.NewInt64CounterConn(conn, []*atomic.Int64{&c.up}, []*atomic.Int64{&c.down})
	return tracked
}

func (t *Tracker) RoutedPacketConnection(ctx context.Context, conn N.PacketConn, metadata adapter.InboundContext, matchedRule adapter.Rule, matchOutbound adapter.Outbound) N.PacketConn {
	if !strings.HasPrefix(metadata.Inbound, NodeTagPrefix) {
		return conn
	}
	key := Key{Inbound: metadata.Inbound, User: metadata.User}
	tracked := &trackedPacketConn{tracker: t, key: key}
	c, id, ok := t.register(key, func() { conn.Close() })
	if !ok {
		conn.Close()
		return conn
	}
	tracked.id = id
	tracked.PacketConn = bufio.NewInt64CounterPacketConn(conn, []*atomic.Int64{&c.up}, nil, []*atomic.Int64{&c.down}, nil)
	return tracked
}

// RoutedFlow 只用于 TUN 入站，Agent 用不到。
func (t *Tracker) RoutedFlow(ctx context.Context, metadata adapter.InboundContext, matchedRule adapter.Rule, matchOutbound adapter.Outbound) tun.FlowTracker {
	return nil
}

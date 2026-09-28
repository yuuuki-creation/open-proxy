package core

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"net"
	"net/netip"
	"slices"
	"sync"
	"sync/atomic"
	"time"

	"github.com/sagernet/sing-box/adapter"
	"github.com/sagernet/sing/common/buf"
	"github.com/sagernet/sing/common/bufio"
	M "github.com/sagernet/sing/common/metadata"
	N "github.com/sagernet/sing/common/network"
)

// 目标地址检查（技术债 agent-5）：代理用户不能经节点访问节点服务器本机和内网。
// 所有入站（包括 Mieru 和自建落地）的连接最后都交给分发出站，检查就放在那里：
//   - 目标是 IP：不允许就拒绝
//   - 目标是域名、走直连：先解析，去掉不允许的地址，全不允许就拒绝；用剩下的地址拨号，
//     拨号时不再解析（防 DNS 重绑定：两次解析结果不同，第二次指向内网）
//   - 走落地出口：只检查 IP 字面量，域名由落地解析；Agent 自己连落地服务器不受限制（落地可以在本机）
//   - UDP 每个包都查：一个 UDP 会话可以发往不同的目标
// 面板和期望状态里没有开关。只有命令行参数 -allow-private-targets（只给测试用：测试机上能连的目标都在本机）
// 能关掉检查，见 Options.AllowPrivateTargets。

// errForbidden 表示目标是节点服务器本机或内网地址。
var errForbidden = errors.New("不允许访问节点服务器本机和内网地址")

// netip 能直接判断的几类（回环、私有、链路本地、未指定、组播）以外，还要拦的网段。
var forbiddenPrefixes = []netip.Prefix{
	netip.MustParsePrefix("0.0.0.0/8"),          // 「本网络」，Linux 上连它等于连本机
	netip.MustParsePrefix("100.64.0.0/10"),      // 运营商级 NAT（RFC 6598），云服务商常用作内网
	netip.MustParsePrefix("255.255.255.255/32"), // 受限广播
}

const (
	// 本机网卡地址的缓存时间
	localAddrsTTL = time.Minute
	// UDP 包的目标是域名时，解析结果在这个 UDP 会话里缓存多久
	packetResolveTTL = time.Minute
	// 拒绝日志：每个时间窗最多逐条记这么多，多出来的只记个数
	rejectLogWindow = 10 * time.Second
	rejectLogBurst  = 10
)

// guard 检查连接的目标地址。
type guard struct {
	dns      adapter.DNSRouter
	allowAll bool // -allow-private-targets：不检查，什么目标都放行
	local    localAddrs
	rejects  rejectLog
}

// allowed 判断能不能访问这个 IP。
func (g *guard) allowed(addr netip.Addr) bool {
	addr = addr.Unmap()
	if !addr.IsValid() || addr.IsLoopback() || addr.IsPrivate() || addr.IsUnspecified() ||
		addr.IsLinkLocalUnicast() || addr.IsMulticast() || addr.IsLinkLocalMulticast() || addr.IsInterfaceLocalMulticast() {
		return false
	}
	for _, prefix := range forbiddenPrefixes {
		if prefix.Contains(addr) {
			return false
		}
	}
	return !g.local.contains(addr)
}

// check 检查一条连接的目标，direct 表示走直连。目标是域名且走直连时，返回解析好、允许访问的地址，
// 调用方只能用这些地址拨号；其他情况返回 nil。
func (g *guard) check(ctx context.Context, destination M.Socksaddr, direct bool) ([]netip.Addr, error) {
	if g.allowAll {
		return nil, nil // 域名照常交给出站自己解析
	}
	switch {
	case destination.IsIP():
		if !g.allowed(destination.Addr) {
			return nil, fmt.Errorf("%w：%s", errForbidden, destination)
		}
		return nil, nil
	case destination.IsDomain():
		if !direct {
			return nil, nil
		}
		return g.resolve(ctx, destination.Fqdn)
	}
	return nil, fmt.Errorf("目标地址 %q 不对", destination)
}

// filter 从已经解析好的地址里去掉不允许的（sing-box 自己解析过时用）。
func (g *guard) filter(destination M.Socksaddr, addrs []netip.Addr) ([]netip.Addr, error) {
	if g.allowAll {
		return addrs, nil
	}
	allowed := make([]netip.Addr, 0, len(addrs))
	for _, addr := range addrs {
		if g.allowed(addr) {
			allowed = append(allowed, addr.Unmap())
		}
	}
	if len(allowed) == 0 {
		return nil, fmt.Errorf("%w：%s 解析到 %v", errForbidden, destination, addrs)
	}
	// IPv4 在前：节点服务器只保证有 IPv4
	slices.SortStableFunc(allowed, func(a, b netip.Addr) int {
		switch {
		case a.Is4() == b.Is4():
			return 0
		case a.Is4():
			return -1
		}
		return 1
	})
	return allowed, nil
}

// resolve 用 sing-box 的 DNS（和直连出站自己解析时一样）解析域名，返回允许访问的地址。
func (g *guard) resolve(ctx context.Context, domain string) ([]netip.Addr, error) {
	addrs, err := g.dns.Lookup(ctx, domain, adapter.DNSQueryOptions{})
	if err != nil {
		return nil, fmt.Errorf("解析 %s: %w", domain, err)
	}
	return g.filter(M.Socksaddr{Fqdn: domain}, addrs)
}

// localAddrs 缓存本机网卡上的所有地址，每分钟刷新一次，不是每个连接（每个 UDP 包）都去读。
type localAddrs struct {
	snapshot atomic.Pointer[localSnapshot]

	mu      sync.Mutex // 刷新时持有
	lastErr string
}

type localSnapshot struct {
	addrs   map[netip.Addr]struct{}
	expires time.Time
}

func (l *localAddrs) contains(addr netip.Addr) bool {
	s := l.snapshot.Load()
	if s == nil || time.Now().After(s.expires) {
		s = l.refresh()
	}
	_, ok := s.addrs[addr]
	return ok
}

// refresh 重读网卡地址。读失败时沿用上一次的结果，同样的错误只记一次日志。
func (l *localAddrs) refresh() *localSnapshot {
	l.mu.Lock()
	defer l.mu.Unlock()
	now := time.Now()
	old := l.snapshot.Load()
	if old != nil && now.Before(old.expires) {
		return old // 别的 goroutine 刚刷新过
	}
	next := &localSnapshot{expires: now.Add(localAddrsTTL)}
	ifaceAddrs, err := net.InterfaceAddrs()
	if err != nil {
		if msg := err.Error(); msg != l.lastErr {
			slog.Warn("读本机网卡地址失败，先用上一次的结果", "err", err)
			l.lastErr = msg
		}
		if old != nil {
			next.addrs = old.addrs
		}
		l.snapshot.Store(next)
		return next
	}
	l.lastErr = ""
	next.addrs = make(map[netip.Addr]struct{}, len(ifaceAddrs))
	for _, a := range ifaceAddrs {
		if ipNet, ok := a.(*net.IPNet); ok {
			if addr, ok := netip.AddrFromSlice(ipNet.IP); ok {
				next.addrs[addr.Unmap()] = struct{}{}
			}
		}
	}
	l.snapshot.Store(next)
	return next
}

// rejectLog 用调试级别记被拒绝的连接。拒绝可能很多（例如有人扫内网），每 10 秒最多逐条记 10 条，
// 多出来的只在下一个时间窗开头记一个总数。
type rejectLog struct {
	mu         sync.Mutex
	windowEnd  time.Time
	count      int
	suppressed int
}

func (r *rejectLog) log(inbound, user, network string, destination M.Socksaddr, err error) {
	if !slog.Default().Enabled(context.Background(), slog.LevelDebug) {
		return
	}
	r.mu.Lock()
	now := time.Now()
	if now.After(r.windowEnd) {
		if r.suppressed > 0 {
			slog.Debug("前一段时间还拒绝了一些连接，没有逐条记", "count", r.suppressed)
		}
		r.windowEnd = now.Add(rejectLogWindow)
		r.count = 0
		r.suppressed = 0
	}
	r.count++
	if r.count > rejectLogBurst {
		r.suppressed++
		r.mu.Unlock()
		return
	}
	r.mu.Unlock()
	slog.Debug("拒绝连接", "inbound", inbound, "user", user, "network", network, "destination", destination, "err", err)
}

// guardedPacketConn 包在出站的 UDP 连接外面，每个包发出之前检查目标：不允许的包丢掉，
// 整个 UDP 会话照常；走直连时目标是域名的，解析后发往允许的地址。
//
// 只实现 N.NetPacketConn 和余量接口，不暴露上游（没有 Upstream、WriterReplaceable）：
// sing 的拷贝循环会顺着这些接口找到最里层的写入端直接写，那样就绕过了检查。
type guardedPacketConn struct {
	conn    N.NetPacketConn
	guard   *guard
	direct  bool
	ctx     context.Context
	inbound string
	user    string

	mu       sync.Mutex
	resolved map[string]resolvedAddr
}

type resolvedAddr struct {
	addr    netip.Addr
	err     error
	expires time.Time
}

var _ N.NetPacketConn = (*guardedPacketConn)(nil)

func newGuardedPacketConn(ctx context.Context, conn net.PacketConn, g *guard, direct bool, metadata *adapter.InboundContext) *guardedPacketConn {
	c := &guardedPacketConn{conn: bufio.NewPacketConn(conn), guard: g, direct: direct, ctx: ctx}
	if metadata != nil {
		c.inbound, c.user = metadata.Inbound, metadata.User
	}
	return c
}

// target 算出一个包实际发往的地址；不允许时返回错误。
func (c *guardedPacketConn) target(destination M.Socksaddr) (M.Socksaddr, error) {
	if destination.IsIP() {
		if !c.guard.allowed(destination.Addr) {
			return M.Socksaddr{}, fmt.Errorf("%w：%s", errForbidden, destination)
		}
		return destination, nil
	}
	if !c.direct || !destination.IsDomain() {
		return destination, nil
	}
	addr, err := c.lookup(destination.Fqdn)
	if err != nil {
		return M.Socksaddr{}, err
	}
	return M.SocksaddrFrom(addr, destination.Port), nil
}

// lookup 解析 UDP 包目标里的域名，结果（包括失败）缓存一分钟。
func (c *guardedPacketConn) lookup(domain string) (netip.Addr, error) {
	now := time.Now()
	c.mu.Lock()
	if r, ok := c.resolved[domain]; ok && now.Before(r.expires) {
		c.mu.Unlock()
		return r.addr, r.err
	}
	c.mu.Unlock()
	var r resolvedAddr
	addrs, err := c.guard.resolve(c.ctx, domain)
	if err != nil {
		r.err = err
	} else {
		r.addr = addrs[0]
	}
	r.expires = now.Add(packetResolveTTL)
	c.mu.Lock()
	if c.resolved == nil {
		c.resolved = make(map[string]resolvedAddr)
	}
	c.resolved[domain] = r
	c.mu.Unlock()
	return r.addr, r.err
}

func (c *guardedPacketConn) WritePacket(buffer *buf.Buffer, destination M.Socksaddr) error {
	target, err := c.target(destination)
	if err != nil {
		buffer.Release()
		c.guard.rejects.log(c.inbound, c.user, N.NetworkUDP, destination, err)
		return nil // 丢掉这个包，UDP 会话照常
	}
	return c.conn.WritePacket(buffer, target)
}

func (c *guardedPacketConn) WriteTo(p []byte, addr net.Addr) (int, error) {
	destination := M.SocksaddrFromNet(addr)
	target, err := c.target(destination)
	if err != nil {
		c.guard.rejects.log(c.inbound, c.user, N.NetworkUDP, destination, err)
		return len(p), nil
	}
	if target.IsIP() {
		return c.conn.WriteTo(p, target.UDPAddr())
	}
	return c.conn.WriteTo(p, target)
}

func (c *guardedPacketConn) ReadPacket(buffer *buf.Buffer) (M.Socksaddr, error) {
	return c.conn.ReadPacket(buffer)
}

func (c *guardedPacketConn) ReadFrom(p []byte) (int, net.Addr, error) {
	return c.conn.ReadFrom(p)
}

func (c *guardedPacketConn) Close() error                       { return c.conn.Close() }
func (c *guardedPacketConn) LocalAddr() net.Addr                { return c.conn.LocalAddr() }
func (c *guardedPacketConn) SetDeadline(t time.Time) error      { return c.conn.SetDeadline(t) }
func (c *guardedPacketConn) SetReadDeadline(t time.Time) error  { return c.conn.SetReadDeadline(t) }
func (c *guardedPacketConn) SetWriteDeadline(t time.Time) error { return c.conn.SetWriteDeadline(t) }

// 写入端要预留的头尾空间照实报给拷贝循环（例如经落地出口时 SOCKS5 要在前面加头），否则会越界。
func (c *guardedPacketConn) FrontHeadroom() int { return N.CalculateFrontHeadroom(c.conn) }
func (c *guardedPacketConn) RearHeadroom() int  { return N.CalculateRearHeadroom(c.conn) }

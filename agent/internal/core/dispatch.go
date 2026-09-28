package core

import (
	"context"
	"errors"
	"fmt"
	"net"
	"net/netip"
	"strings"
	"sync"

	"github.com/sagernet/sing-box/adapter"
	"github.com/sagernet/sing-box/adapter/outbound"
	"github.com/sagernet/sing-box/log"
	M "github.com/sagernet/sing/common/metadata"
	N "github.com/sagernet/sing/common/network"
	"github.com/sagernet/sing/service"

	"github.com/yuuuki-creation/open-proxy/agent/internal/tracker"
)

// 分发出站：路由的默认出站。拨号时从连接的上下文里取入站 tag，查对应表，
// 交给 exit-<ID> 或直连出站。sing-box 的路由规则不能在运行时修改，
// 而节点和出口随时增删，所以用它代替「每个入站一条路由规则」。
// 见 architecture.md「落地出口」。所有入站的连接都经过这里，目标地址的检查也放在这里（guard.go）。
const (
	dispatchType = "op-dispatch"
	dispatchTag  = "dispatch"
	directTag    = "direct"
)

type dispatchOptions struct{}

// routes 是分发出站查的表，随期望状态整体替换。
type routes struct {
	mu         sync.RWMutex
	outboundOf map[string]string       // 节点入站 tag -> 出站 tag；没有的走直连
	landing    map[netip.Addr]struct{} // 自建落地放行的来源 IP
}

func newRoutes() *routes {
	return &routes{
		outboundOf: make(map[string]string),
		landing:    make(map[netip.Addr]struct{}),
	}
}

func (r *routes) setOutbounds(outboundOf map[string]string) {
	copied := make(map[string]string, len(outboundOf))
	for k, v := range outboundOf {
		copied[k] = v
	}
	r.mu.Lock()
	r.outboundOf = copied
	r.mu.Unlock()
}

func (r *routes) setLandingSources(addrs []netip.Addr) {
	set := make(map[netip.Addr]struct{}, len(addrs))
	for _, a := range addrs {
		set[a.Unmap()] = struct{}{}
	}
	r.mu.Lock()
	r.landing = set
	r.mu.Unlock()
}

// pick 决定这条连接走哪个出站。
func (r *routes) pick(metadata *adapter.InboundContext, access *tracker.Tracker) (string, error) {
	switch {
	case metadata.Inbound == LandingTag:
		source := metadata.Source.Addr.Unmap()
		r.mu.RLock()
		_, ok := r.landing[source]
		r.mu.RUnlock()
		if !ok {
			return "", fmt.Errorf("来源 %s 不在自建落地的放行名单里", source)
		}
		return directTag, nil
	case strings.HasPrefix(metadata.Inbound, tracker.NodeTagPrefix):
		// 追踪层已经拒绝过的连接，这里就不必再去拨目标地址
		if !access.Allowed(metadata.Inbound, metadata.User) {
			return "", fmt.Errorf("用户 %s 不在节点 %s 的放行名单里", metadata.User, metadata.Inbound)
		}
		r.mu.RLock()
		out, ok := r.outboundOf[metadata.Inbound]
		r.mu.RUnlock()
		if !ok {
			return directTag, nil
		}
		return out, nil
	}
	return "", fmt.Errorf("不认识的入站 %q", metadata.Inbound)
}

type dispatchOutbound struct {
	outbound.Adapter
	manager     adapter.OutboundManager
	connections adapter.ConnectionManager
	routes      *routes
	access      *tracker.Tracker
	guard       *guard
}

// 实现这两个接口，路由就把连接直接交给分发出站（sing-box route.go 的 routeConnection），
// 可以在拨号之前检查、拒绝，再交给连接管理去拨号和转发。
var (
	_ adapter.ConnectionHandler       = (*dispatchOutbound)(nil)
	_ adapter.PacketConnectionHandler = (*dispatchOutbound)(nil)
)

func registerDispatch(registry *outbound.Registry, r *routes, access *tracker.Tracker, allowPrivateTargets bool) {
	outbound.Register[dispatchOptions](registry, dispatchType, func(ctx context.Context, router adapter.Router, logger log.ContextLogger, tag string, _ dispatchOptions) (adapter.Outbound, error) {
		manager := service.FromContext[adapter.OutboundManager](ctx)
		connections := service.FromContext[adapter.ConnectionManager](ctx)
		dns := service.FromContext[adapter.DNSRouter](ctx)
		if manager == nil || connections == nil || dns == nil {
			return nil, errors.New("分发出站拿不到出站管理、连接管理或 DNS")
		}
		return &dispatchOutbound{
			Adapter:     outbound.NewAdapter(dispatchType, tag, []string{N.NetworkTCP, N.NetworkUDP}, nil),
			manager:     manager,
			connections: connections,
			routes:      r,
			access:      access,
			guard:       &guard{dns: dns, allowAll: allowPrivateTargets},
		}, nil
	})
}

// NewConnection 在拨号之前检查（放行名单、目标地址），通过了再交给连接管理拨号和转发。
// 被拒绝的连接按失败关掉，只打调试日志：交给连接管理再失败的话，sing-box 每条都会打错误日志，会刷屏。
func (d *dispatchOutbound) NewConnection(ctx context.Context, conn net.Conn, metadata adapter.InboundContext, onClose N.CloseHandlerFunc) {
	if err := d.prepare(ctx, &metadata); err != nil {
		d.guard.rejects.log(metadata.Inbound, metadata.User, N.NetworkTCP, metadata.Destination, err)
		N.CloseOnHandshakeFailure(conn, onClose, err)
		return
	}
	d.connections.NewConnection(ctx, d, conn, metadata, onClose)
}

// NewPacketConnection 同 NewConnection。检查的是第一个包的目标，之后每个包由 guardedPacketConn 检查。
func (d *dispatchOutbound) NewPacketConnection(ctx context.Context, conn N.PacketConn, metadata adapter.InboundContext, onClose N.CloseHandlerFunc) {
	if err := d.prepare(ctx, &metadata); err != nil {
		d.guard.rejects.log(metadata.Inbound, metadata.User, N.NetworkUDP, metadata.Destination, err)
		N.CloseOnHandshakeFailure(conn, onClose, err)
		return
	}
	d.connections.NewPacketConnection(ctx, d, conn, metadata, onClose)
}

// prepare 选出口、检查目标。走直连、目标是域名时，把解析好的允许地址填进 DestinationAddresses：
// 连接管理看到它就按这些地址拨号（再经过 DialContext、ListenPacket），不会再解析一次。
func (d *dispatchOutbound) prepare(ctx context.Context, metadata *adapter.InboundContext) error {
	tag, err := d.routes.pick(metadata, d.access)
	if err != nil {
		return err
	}
	direct := tag == directTag
	if len(metadata.DestinationAddresses) > 0 {
		// sing-box 已经解析过（现在没有这样的路由规则，以防以后加了）：也要去掉不允许的
		if !direct {
			return nil // 按地址拨号时 DialContext 会逐个检查
		}
		addrs, err := d.guard.filter(metadata.Destination, metadata.DestinationAddresses)
		if err != nil {
			return err
		}
		metadata.DestinationAddresses = addrs
		return nil
	}
	addrs, err := d.guard.check(ctx, metadata.Destination, direct)
	if err != nil {
		return err
	}
	if len(addrs) > 0 {
		metadata.DestinationAddresses = addrs
	}
	return nil
}

// target 按连接的入站信息找到真正要用的出站，direct 表示是直连。
// 落地出口还没建成功时照样指向它，拨号会失败：绝不退回直连，免得流量从错误的出口出去。
func (d *dispatchOutbound) target(ctx context.Context) (adapter.Outbound, *adapter.InboundContext, bool, error) {
	metadata := adapter.ContextFrom(ctx)
	if metadata == nil {
		return nil, nil, false, errors.New("分发出站拿不到连接的入站信息")
	}
	tag, err := d.routes.pick(metadata, d.access)
	if err != nil {
		return nil, nil, false, err
	}
	out, ok := d.manager.Outbound(tag)
	if !ok {
		return nil, nil, false, fmt.Errorf("出站 %s 不存在", tag)
	}
	return out, metadata, tag == directTag, nil
}

// DialContext 由连接管理调用。目标地址再查一遍：NewConnection 已经查过，这里防的是别的调用路径。
func (d *dispatchOutbound) DialContext(ctx context.Context, network string, destination M.Socksaddr) (net.Conn, error) {
	out, _, direct, err := d.target(ctx)
	if err != nil {
		return nil, err
	}
	addrs, err := d.guard.check(ctx, destination, direct)
	if err != nil {
		return nil, err
	}
	if len(addrs) > 0 {
		// 直连、目标是域名还没解析：正常走不到这里（NewConnection 已经解析好了），兜底用解析好的地址拨
		return N.DialSerial(ctx, out, network, destination, addrs)
	}
	return out.DialContext(ctx, network, destination)
}

// ListenPacket 由连接管理调用。第一个包的目标再查一遍，返回的连接每个包都检查。
func (d *dispatchOutbound) ListenPacket(ctx context.Context, destination M.Socksaddr) (net.PacketConn, error) {
	out, metadata, direct, err := d.target(ctx)
	if err != nil {
		return nil, err
	}
	addrs, err := d.guard.check(ctx, destination, direct)
	if err != nil {
		return nil, err
	}
	var conn net.PacketConn
	if len(addrs) > 0 {
		// 同 DialContext 的兜底
		conn, _, err = N.ListenSerial(ctx, out, destination, addrs)
	} else {
		conn, err = out.ListenPacket(ctx, destination)
	}
	if err != nil {
		return nil, err
	}
	if d.guard.allowAll {
		return conn, nil
	}
	return newGuardedPacketConn(ctx, conn, d.guard, direct, metadata), nil
}

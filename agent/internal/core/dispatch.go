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
// 见 architecture.md「落地出口」。
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
	manager adapter.OutboundManager
	routes  *routes
	access  *tracker.Tracker
}

func registerDispatch(registry *outbound.Registry, r *routes, access *tracker.Tracker) {
	outbound.Register[dispatchOptions](registry, dispatchType, func(ctx context.Context, router adapter.Router, logger log.ContextLogger, tag string, _ dispatchOptions) (adapter.Outbound, error) {
		manager := service.FromContext[adapter.OutboundManager](ctx)
		if manager == nil {
			return nil, errors.New("分发出站拿不到出站管理器")
		}
		return &dispatchOutbound{
			Adapter: outbound.NewAdapter(dispatchType, tag, []string{N.NetworkTCP, N.NetworkUDP}, nil),
			manager: manager,
			routes:  r,
			access:  access,
		}, nil
	})
}

// target 按连接的入站信息找到真正要用的出站。
// 落地出口还没建成功时照样指向它，拨号会失败：绝不退回直连，免得流量从错误的出口出去。
func (d *dispatchOutbound) target(ctx context.Context) (adapter.Outbound, error) {
	metadata := adapter.ContextFrom(ctx)
	if metadata == nil {
		return nil, errors.New("分发出站拿不到连接的入站信息")
	}
	tag, err := d.routes.pick(metadata, d.access)
	if err != nil {
		return nil, err
	}
	out, ok := d.manager.Outbound(tag)
	if !ok {
		return nil, fmt.Errorf("出站 %s 不存在", tag)
	}
	return out, nil
}

func (d *dispatchOutbound) DialContext(ctx context.Context, network string, destination M.Socksaddr) (net.Conn, error) {
	out, err := d.target(ctx)
	if err != nil {
		return nil, err
	}
	return out.DialContext(ctx, network, destination)
}

func (d *dispatchOutbound) ListenPacket(ctx context.Context, destination M.Socksaddr) (net.PacketConn, error) {
	out, err := d.target(ctx)
	if err != nil {
		return nil, err
	}
	return out.ListenPacket(ctx, destination)
}

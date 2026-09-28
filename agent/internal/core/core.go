// Package core 管住内嵌的 sing-box 实例：启动；按 tag 建、换、删入站和出站；
// 维护分发出站的对应表。Mieru 节点不是 sing-box 的入站，也在这里按同样的 tag 管
// （internal/mieru），对调用方来说和其他入站一样。本身不加锁，只在状态管理的 goroutine 里调用
// （architecture.md「一把锁串行化所有变更」）。
package core

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"net/netip"

	box "github.com/sagernet/sing-box"
	"github.com/sagernet/sing-box/adapter"
	C "github.com/sagernet/sing-box/constant"
	"github.com/sagernet/sing-box/include"
	"github.com/sagernet/sing-box/option"
	"github.com/sagernet/sing/service"
	"github.com/sagernet/sing/service/pause"

	"github.com/yuuuki-creation/open-proxy/agent/internal/mieru"
	"github.com/yuuuki-creation/open-proxy/agent/internal/tracker"
)

// Core 是一个运行中的 sing-box 实例，加上 Mieru 节点的服务端。
type Core struct {
	ctx      context.Context
	box      *box.Box
	inbounds adapter.InboundRegistry // 校验入站配置时用它只构造、不启动
	routes   *routes
	mierus   map[string]*mieru.Server // Mieru 节点，按入站 tag；连接交给 sing-box 路由
}

// Start 启动一个没有任何入站的 sing-box 实例，出站只有直连和分发；入站和落地出口
// 之后按期望状态逐个加。追踪层要在启动之前挂上。
func Start(ctx context.Context, tr *tracker.Tracker) (*Core, error) {
	r := newRoutes()
	inboundRegistry := include.InboundRegistry()
	outboundRegistry := include.OutboundRegistry()
	registerDispatch(outboundRegistry, r, tr)

	// context 的准备顺序（原型里踩过的坑）：box.New 会把各种服务注册进 context 里的
	// 服务注册表，注册表存在就复用。必须先建好注册表和 pause 管理器再交给 box.New，
	// 否则运行时调用 InboundManager.Create 会空指针崩溃。
	bctx := box.Context(ctx,
		inboundRegistry,
		outboundRegistry,
		include.EndpointRegistry(),
		include.DNSTransportRegistry(),
		include.ServiceRegistry(),
		include.CertificateProviderRegistry(),
	)
	bctx = service.ContextWithDefaultRegistry(bctx)
	bctx = pause.WithDefaultManager(bctx)

	instance, err := box.New(box.Options{
		Context: bctx,
		Options: option.Options{
			// 不带时间戳，由 journald 记录时间；也不带颜色码，journald 里显示成乱码
			Log: &option.LogOptions{Level: "warn", DisableColor: true},
			Outbounds: []option.Outbound{
				{Type: C.TypeDirect, Tag: directTag, Options: &option.DirectOutboundOptions{}},
				{Type: dispatchType, Tag: dispatchTag, Options: &dispatchOptions{}},
			},
			// 所有连接都交给分发出站，由它按入站查表选出口；路由规则不能在运行时改，所以不写规则
			Route: &option.RouteOptions{Final: dispatchTag},
		},
	})
	if err != nil {
		return nil, fmt.Errorf("创建 sing-box 实例: %w", err)
	}
	instance.Router().AppendTracker(tr)
	if err := instance.Start(); err != nil {
		instance.Close()
		return nil, fmt.Errorf("启动 sing-box 实例: %w", err)
	}
	return &Core{ctx: bctx, box: instance, inbounds: inboundRegistry, routes: r, mierus: make(map[string]*mieru.Server)}, nil
}

// Close 先停 Mieru 节点（它们的连接要经过 sing-box），再停 sing-box 实例。
func (c *Core) Close() error {
	for tag := range c.mierus {
		c.stopMieru(tag)
	}
	return c.box.Close()
}

// InboundConfig 是建一个入站要的类型和选项。
type InboundConfig struct {
	Type    string
	Options any
}

// ErrInboundStopped 表示入站重建失败，按原来的配置也没建回来，这个入站已经停了。
var ErrInboundStopped = errors.New("入站已停止")

// SetInbound 按 tag 建入站；tag 已存在时替换。用于新建和端口变了的入站：
// 先启动新的，成功后才关旧的，新的起不来时旧的原样保留（sing-box 替换同 tag 入站也是这样）。
// 节点换了协议（sing-box 入站和 Mieru 之间）也一样：新的起来了才停掉原来那种。
// 端口没变的入站用 RebuildInbound。
func (c *Core) SetInbound(tag string, cfg InboundConfig) error {
	if cfg.Type == mieru.Type {
		return c.setMieru(tag, cfg)
	}
	logger := c.box.LogFactory().NewLogger("inbound/" + cfg.Type + "[" + tag + "]")
	if err := c.box.Inbound().Create(c.ctx, c.box.Router(), logger, tag, cfg.Type, cfg.Options); err != nil {
		return fmt.Errorf("创建入站 %s: %w", tag, err)
	}
	c.stopMieru(tag)
	return nil
}

// setMieru 启动 Mieru 节点的服务端；同一个 tag 原来有 Mieru 服务端或 sing-box 入站的，新的起来后再停掉。
func (c *Core) setMieru(tag string, cfg InboundConfig) error {
	opts, err := mieruOptions(cfg)
	if err != nil {
		return err
	}
	server, err := mieru.Start(c.ctx, c.box.Router(), tag, opts)
	if err != nil {
		return fmt.Errorf("创建入站 %s: %w", tag, err)
	}
	c.stopMieru(tag)
	c.mierus[tag] = server
	if err := c.removeBoxInbound(tag); err != nil {
		slog.Warn("节点换成 Mieru 后，移除原来的入站失败", "inbound", tag, "err", err)
	}
	return nil
}

// stopMieru 停掉 tag 对应的 Mieru 服务端，这个节点上的连接全部断开；不存在时什么都不做。
func (c *Core) stopMieru(tag string) {
	if server, ok := c.mierus[tag]; ok {
		server.Close()
		delete(c.mierus, tag)
	}
}

func mieruOptions(cfg InboundConfig) (*mieru.Options, error) {
	opts, ok := cfg.Options.(*mieru.Options)
	if !ok || opts == nil {
		return nil, fmt.Errorf("入站类型是 %s，配置却是 %T", cfg.Type, cfg.Options)
	}
	return opts, nil
}

// UpdateMieruUsers 不重建，直接换掉 Mieru 节点的用户列表。cfg 是这个节点的新配置，只用其中的用户，
// 端口要和运行中的一样。有用户的密码变了时返回 mieru.ErrPasswordChanged，由调用方改为重建。
func (c *Core) UpdateMieruUsers(tag string, cfg InboundConfig) error {
	server, ok := c.mierus[tag]
	if !ok {
		return fmt.Errorf("找不到 Mieru 节点 %s", tag)
	}
	opts, err := mieruOptions(cfg)
	if err != nil {
		return err
	}
	return server.UpdateUsers(opts.Users)
}

// RebuildInbound 用 next 重建端口没变的入站，prev 是它现在运行的配置。
//
// 端口没变时新入站会因为端口被旧的占着而起不来，只能先删旧的再建。为了做到
// 「失败项保持原样」（protocol.md「应用规则」）：删之前先构造一遍新入站来校验，
// 证书、REALITY 密钥、Shadowsocks 密钥等都在构造时解析，不通过就不动旧的；
// 删掉旧的以后新的还是起不来（比如端口被别的程序占了），就按 prev 建回去。
// 两次都失败时返回的错误包含 ErrInboundStopped。
func (c *Core) RebuildInbound(tag string, next, prev InboundConfig) error {
	if err := c.checkInbound(tag, next); err != nil {
		return fmt.Errorf("校验新配置: %w", err)
	}
	// sing-box 先把入站从管理器里摘掉再关：关的时候出错，它也已经不在了，照样往下建
	removeErr := c.RemoveInbound(tag)
	err := c.SetInbound(tag, next)
	if err == nil {
		return nil
	}
	err = errors.Join(removeErr, err)
	if restoreErr := c.SetInbound(tag, prev); restoreErr != nil {
		return fmt.Errorf("%w：新配置启动失败（%v），按原来的配置恢复也失败（%v）", ErrInboundStopped, err, restoreErr)
	}
	return fmt.Errorf("新配置启动失败，已按原来的配置恢复: %w", err)
}

// checkInbound 按配置构造一个入站但不启动，构造完就关掉：只校验配置，不占端口。
func (c *Core) checkInbound(tag string, cfg InboundConfig) error {
	if cfg.Type == mieru.Type {
		opts, err := mieruOptions(cfg)
		if err != nil {
			return err
		}
		return mieru.Check(opts)
	}
	logger := c.box.LogFactory().NewLogger("inbound/" + cfg.Type + "[" + tag + "]")
	inbound, err := c.inbounds.Create(c.ctx, c.box.Router(), logger, tag, cfg.Type, cfg.Options)
	if err != nil {
		return err
	}
	// 没启动过，关掉只是释放构造时建的对象
	inbound.Close()
	return nil
}

// RemoveInbound 删除入站（sing-box 入站或 Mieru 服务端）；不存在时什么都不做。
func (c *Core) RemoveInbound(tag string) error {
	c.stopMieru(tag)
	return c.removeBoxInbound(tag)
}

func (c *Core) removeBoxInbound(tag string) error {
	manager := c.box.Inbound()
	if _, ok := manager.Get(tag); !ok {
		return nil
	}
	if err := manager.Remove(tag); err != nil {
		return fmt.Errorf("移除入站 %s: %w", tag, err)
	}
	return nil
}

// SetOutbound 按 tag 建出站；tag 已存在时替换（新的起来才关旧的，失败时旧的保留）。
func (c *Core) SetOutbound(tag, outboundType string, options any) error {
	logger := c.box.LogFactory().NewLogger("outbound/" + outboundType + "[" + tag + "]")
	if err := c.box.Outbound().Create(c.ctx, c.box.Router(), logger, tag, outboundType, options); err != nil {
		return fmt.Errorf("创建出站 %s: %w", tag, err)
	}
	return nil
}

// RemoveOutbound 删除出站；不存在时什么都不做。
func (c *Core) RemoveOutbound(tag string) error {
	manager := c.box.Outbound()
	if _, ok := manager.Outbound(tag); !ok {
		return nil
	}
	if err := manager.Remove(tag); err != nil {
		return fmt.Errorf("移除出站 %s: %w", tag, err)
	}
	return nil
}

// UpdateShadowsocksUsers 不重建入站，直接换掉 Shadowsocks 入站的用户列表（原型 V4 验证过）。
func (c *Core) UpdateShadowsocksUsers(tag string, names, keys []string) error {
	inbound, ok := c.box.Inbound().Get(tag)
	if !ok {
		return fmt.Errorf("找不到入站 %s", tag)
	}
	managed, ok := inbound.(adapter.ManagedSSMServer)
	if !ok {
		return fmt.Errorf("入站 %s 不支持热更新用户", tag)
	}
	return managed.UpdateUsers(names, keys)
}

// SetRoutes 整体替换「节点入站 tag → 出站 tag」的对应表。表里没有的节点走直连。
func (c *Core) SetRoutes(outboundOf map[string]string) {
	c.routes.setOutbounds(outboundOf)
}

// SetLandingSources 整体替换自建落地放行的来源 IP。
func (c *Core) SetLandingSources(addrs []netip.Addr) {
	c.routes.setLandingSources(addrs)
}

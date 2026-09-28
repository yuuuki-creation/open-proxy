package main

import (
	"context"
	"fmt"
	"net/netip"
	"sync"

	box "github.com/sagernet/sing-box"
	"github.com/sagernet/sing-box/adapter"
	C "github.com/sagernet/sing-box/constant"
	"github.com/sagernet/sing-box/include"
	"github.com/sagernet/sing-box/option"
	"github.com/sagernet/sing/common/json/badoption"
	"github.com/sagernet/sing/service"
	"github.com/sagernet/sing/service/pause"
)

// 入站 tag，统计和增删用户都按它来
const (
	tagVLESS      = "vless-in"
	tagHysteria2  = "hy2-in"
	tagAnyTLS     = "anytls-in"
	tagShadowsock = "ss-in"
)

type ServerConfig struct {
	Creds         *Creds
	BasePort      uint16 // 四个入站依次用 BasePort .. BasePort+3
	RealityTarget string // REALITY 伪装目标，形如 www.cloudflare.com
	SSMethod      string
	CertHost      string // 自签证书里的名字，Hysteria2 和 AnyTLS 的 SNI
	LogLevel      string
	SSManaged     bool // Shadowsocks 入站是否声明为 managed（可试运行时改用户）
}

func (c ServerConfig) portFor(offset uint16) uint16 { return c.BasePort + offset }

func listenAnyAddr() *badoption.Addr {
	addr := badoption.Addr(netip.MustParseAddr("0.0.0.0"))
	return &addr
}

func ptr[T any](v T) *T { return &v }

func (c ServerConfig) realityTLS() *option.InboundTLSOptions {
	return &option.InboundTLSOptions{
		Enabled:    true,
		ServerName: c.RealityTarget,
		Reality: &option.InboundRealityOptions{
			Enabled: true,
			Handshake: option.InboundRealityHandshakeOptions{
				ServerOptions: option.ServerOptions{Server: c.RealityTarget, ServerPort: 443},
			},
			PrivateKey: c.Creds.RealityPrivateKey,
			ShortID:    badoption.Listable[string]{c.Creds.RealityShortID},
		},
	}
}

func (c ServerConfig) selfSignedTLS(alpn []string) *option.InboundTLSOptions {
	return &option.InboundTLSOptions{
		Enabled:         true,
		ServerName:      c.CertHost,
		ALPN:            alpn,
		CertificatePath: c.Creds.CertPath,
		KeyPath:         c.Creds.KeyPath,
	}
}

// inboundSpecs 按当前用户列表生成四个入站的配置。
// 增删用户就是用新的用户列表重新生成，再按同一个 tag 重建入站。
func inboundSpecs(c ServerConfig, users []User) []option.Inbound {
	vlessUsers := make([]option.VLESSUser, 0, len(users))
	hy2Users := make([]option.Hysteria2User, 0, len(users))
	anytlsUsers := make([]option.AnyTLSUser, 0, len(users))
	ssUsers := make([]option.ShadowsocksUser, 0, len(users))
	for _, u := range users {
		vlessUsers = append(vlessUsers, option.VLESSUser{Name: u.Name, UUID: u.UUID, Flow: "xtls-rprx-vision"})
		hy2Users = append(hy2Users, option.Hysteria2User{Name: u.Name, Password: u.Password})
		anytlsUsers = append(anytlsUsers, option.AnyTLSUser{Name: u.Name, Password: u.Password})
		ssUsers = append(ssUsers, option.ShadowsocksUser{Name: u.Name, Password: u.SSKey})
	}
	return []option.Inbound{
		{
			Type: C.TypeVLESS,
			Tag:  tagVLESS,
			Options: &option.VLESSInboundOptions{
				ListenOptions:              option.ListenOptions{Listen: listenAnyAddr(), ListenPort: c.portFor(0)},
				Users:                      vlessUsers,
				InboundTLSOptionsContainer: option.InboundTLSOptionsContainer{TLS: c.realityTLS()},
			},
		},
		{
			Type: C.TypeHysteria2,
			Tag:  tagHysteria2,
			Options: &option.Hysteria2InboundOptions{
				ListenOptions:              option.ListenOptions{Listen: listenAnyAddr(), ListenPort: c.portFor(1)},
				Users:                      hy2Users,
				InboundTLSOptionsContainer: option.InboundTLSOptionsContainer{TLS: c.selfSignedTLS([]string{"h3"})},
			},
		},
		{
			Type: C.TypeAnyTLS,
			Tag:  tagAnyTLS,
			Options: &option.AnyTLSInboundOptions{
				ListenOptions:              option.ListenOptions{Listen: listenAnyAddr(), ListenPort: c.portFor(2)},
				Users:                      anytlsUsers,
				InboundTLSOptionsContainer: option.InboundTLSOptionsContainer{TLS: c.selfSignedTLS(nil)},
			},
		},
		{
			Type: C.TypeShadowsocks,
			Tag:  tagShadowsock,
			Options: &option.ShadowsocksInboundOptions{
				ListenOptions: option.ListenOptions{Listen: listenAnyAddr(), ListenPort: c.portFor(3)},
				Method:        c.SSMethod,
				Password:      c.Creds.SSServerKey,
				Users:         ssUsers,
				Managed:       c.SSManaged,
			},
		},
	}
}

// buildOptions 用 Go 代码直接拼出 sing-box 的配置，不写配置文件。
// 正式 Agent 也走这条路：主控下发期望状态，Agent 转成这里的结构体。
func buildOptions(c ServerConfig, users []User) option.Options {
	return option.Options{
		Log:      &option.LogOptions{Level: c.LogLevel, Timestamp: true},
		Inbounds: inboundSpecs(c, users),
		Outbounds: []option.Outbound{
			{Type: C.TypeDirect, Tag: "direct", Options: &option.DirectOutboundOptions{}},
		},
		Route: &option.RouteOptions{Final: "direct"},
	}
}

// Manager 管住实例和当前用户列表，提供运行时增删用户。
type Manager struct {
	ctx      context.Context
	cfg      ServerConfig
	instance *box.Box
	mu       sync.Mutex
	users    []User
}

// StartServer 起一个内嵌的 sing-box 实例，并挂上统计用的追踪层。
// tracker 传 nil 表示不挂，用来对比「挂了追踪层是否影响 splice 零拷贝」。
//
// 注意 context 的准备顺序：box.New 会把一堆服务（网络管理器、日志工厂、
// 各种 manager）注册进 context 里的服务注册表。注册表存在就复用，所以我们
// 必须自己先建好注册表和 pause 管理器再交给 box.New；否则运行时调用
// InboundManager.Create 时，拿到的 context 里没有这些服务，会空指针崩溃。
func StartServer(ctx context.Context, c ServerConfig, tracker *Tracker) (*Manager, error) {
	users := append([]User(nil), c.Creds.Users...)
	baseCtx := include.Context(ctx)
	baseCtx = service.ContextWithDefaultRegistry(baseCtx)
	baseCtx = pause.WithDefaultManager(baseCtx)
	instance, err := box.New(box.Options{
		Context: baseCtx,
		Options: buildOptions(c, users),
	})
	if err != nil {
		return nil, err
	}
	// 追踪层要在 Start 之前挂上
	if tracker != nil {
		instance.Router().AppendTracker(tracker)
	}
	if err = instance.Start(); err != nil {
		instance.Close()
		return nil, err
	}
	return &Manager{ctx: baseCtx, cfg: c, instance: instance, users: users}, nil
}

func (m *Manager) Close() error { return m.instance.Close() }

func (m *Manager) Users() []User {
	m.mu.Lock()
	defer m.mu.Unlock()
	return append([]User(nil), m.users...)
}

// rebuildInbound 按同一个 tag 删掉入站再建回来，这是 sing-box 里
// 唯一能在运行时改用户列表的通用办法（Shadowsocks 另有 UpdateUsers）。
func (m *Manager) rebuildInbound(spec option.Inbound) error {
	im := m.instance.Inbound()
	if _, ok := im.Get(spec.Tag); ok {
		if err := im.Remove(spec.Tag); err != nil {
			return fmt.Errorf("移除入站 %s: %w", spec.Tag, err)
		}
	}
	logger := m.instance.LogFactory().NewLogger(fmt.Sprintf("inbound/%s[%s]", spec.Type, spec.Tag))
	if err := im.Create(m.ctx, m.instance.Router(), logger, spec.Tag, spec.Type, spec.Options); err != nil {
		return fmt.Errorf("重建入站 %s: %w", spec.Tag, err)
	}
	return nil
}

// RebuildTags 用当前用户列表重建指定的入站；tags 为空表示全部。
func (m *Manager) RebuildTags(tags []string) error {
	m.mu.Lock()
	users := append([]User(nil), m.users...)
	m.mu.Unlock()
	want := make(map[string]bool, len(tags))
	for _, t := range tags {
		want[t] = true
	}
	for _, spec := range inboundSpecs(m.cfg, users) {
		if len(want) > 0 && !want[spec.Tag] {
			continue
		}
		if err := m.rebuildInbound(spec); err != nil {
			return err
		}
	}
	return nil
}

// AddUser 生成一个新用户并重建入站。
func (m *Manager) AddUser(name string, ssKeyLen int, tags []string) (*User, error) {
	u, err := NewUser(name, ssKeyLen)
	if err != nil {
		return nil, err
	}
	m.mu.Lock()
	for _, existing := range m.users {
		if existing.Name == name {
			m.mu.Unlock()
			return nil, fmt.Errorf("用户 %s 已存在", name)
		}
	}
	m.users = append(m.users, *u)
	m.mu.Unlock()
	if err = m.RebuildTags(tags); err != nil {
		return nil, err
	}
	return u, nil
}

// RemoveUser 删掉用户并重建入站。
func (m *Manager) RemoveUser(name string, tags []string) error {
	m.mu.Lock()
	kept := make([]User, 0, len(m.users))
	found := false
	for _, u := range m.users {
		if u.Name == name {
			found = true
			continue
		}
		kept = append(kept, u)
	}
	if !found {
		m.mu.Unlock()
		return fmt.Errorf("没有用户 %s", name)
	}
	m.users = kept
	m.mu.Unlock()
	return m.RebuildTags(tags)
}

// UpdateSSUsers 走 Shadowsocks 自己的运行时接口改用户，不重建入站。
// 用来对比「重建入站」和「协议自带的热更新」两条路。
func (m *Manager) UpdateSSUsers() error {
	in, ok := m.instance.Inbound().Get(tagShadowsock)
	if !ok {
		return fmt.Errorf("找不到入站 %s", tagShadowsock)
	}
	managed, ok := in.(adapter.ManagedSSMServer)
	if !ok {
		return fmt.Errorf("该 Shadowsocks 入站没有实现 ManagedSSMServer（需要 managed 选项）")
	}
	users := m.Users()
	names := make([]string, 0, len(users))
	keys := make([]string, 0, len(users))
	for _, u := range users {
		names = append(names, u.Name)
		keys = append(keys, u.SSKey)
	}
	return managed.UpdateUsers(names, keys)
}

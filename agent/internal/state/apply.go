package state

import (
	"cmp"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"fmt"
	"maps"
	"net/netip"
	"slices"

	C "github.com/sagernet/sing-box/constant"
	"google.golang.org/protobuf/proto"

	"github.com/yuuuki-creation/open-proxy/agent/internal/core"
	"github.com/yuuuki-creation/open-proxy/agent/internal/firewall"
	"github.com/yuuuki-creation/open-proxy/agent/internal/mieru"
	agentv1 "github.com/yuuuki-creation/open-proxy/agent/internal/pb/openproxy/agent/v1"
)

// apply 逐项应用一份期望状态，返回失败项。某一项失败时这一项保持原样，其他项照常生效。
func (m *Manager) apply(ds *agentv1.DesiredState) []*agentv1.ApplyFailure {
	var failures []*agentv1.ApplyFailure
	fail := func(item agentv1.ApplyFailure_Item, id uint64, err error) {
		failures = append(failures, &agentv1.ApplyFailure{Item: item, Id: id, Reason: err.Error()})
	}

	users := make(map[uint64]*agentv1.User, len(ds.GetUsers()))
	for _, u := range ds.GetUsers() {
		users[u.GetId()] = u
	}
	wantNodes := make(map[uint64]bool, len(ds.GetNodes()))
	for _, n := range ds.GetNodes() {
		wantNodes[n.GetId()] = true
	}
	wantExits := make(map[uint64]bool, len(ds.GetExits()))
	for _, e := range ds.GetExits() {
		wantExits[e.GetId()] = true
	}

	// 1. 先更新放行名单：停用、删除用户即时生效，不等后面重建入站（protocol.md「应用规则」）
	nodeUsers := make(map[uint64][]*agentv1.User, len(ds.GetNodes()))
	for _, n := range ds.GetNodes() {
		list, names := m.usersOf(n, users)
		nodeUsers[n.GetId()] = list
		m.tracker.SetAllowed(core.NodeTag(n.GetId()), names)
	}
	for id := range m.current.nodes {
		if !wantNodes[id] {
			m.tracker.SetAllowed(core.NodeTag(id), nil)
		}
	}

	// 2. 落地出口：新增或参数变了的按 tag 建；新的建成功才替换旧的，失败时旧的保留
	for _, e := range ds.GetExits() {
		if err := m.applyExit(e); err != nil {
			fail(agentv1.ApplyFailure_ITEM_EXIT, e.GetId(), err)
		}
	}
	// 分发出站的对应表。出口没建成功时照样指向它：拨号失败，也绝不退回直连
	outboundOf := make(map[string]string)
	for _, n := range ds.GetNodes() {
		exitID := n.GetExitId()
		if exitID == 0 {
			continue
		}
		outboundOf[core.NodeTag(n.GetId())] = core.ExitTag(exitID)
		if !wantExits[exitID] {
			fail(agentv1.ApplyFailure_ITEM_NODE, n.GetId(), fmt.Errorf("落地出口 %d 不在期望状态里", exitID))
		}
	}
	m.core.SetRoutes(outboundOf)

	// 3. 证书：校验通过才换上；不通过报失败，TLS 节点接着用原来的证书
	if err := m.applyCertificate(ds.GetCertificate()); err != nil {
		fail(agentv1.ApplyFailure_ITEM_CERTIFICATE, 0, err)
	}

	// 4. 节点
	for _, n := range ds.GetNodes() {
		if err := m.applyNode(n, nodeUsers[n.GetId()], m.current.cert); err != nil {
			fail(agentv1.ApplyFailure_ITEM_NODE, n.GetId(), err)
		}
	}
	for id := range m.current.nodes {
		if wantNodes[id] {
			continue
		}
		if err := m.core.RemoveInbound(core.NodeTag(id)); err != nil {
			m.log.Warn("删除节点失败", "node", id, "err", err)
		}
		delete(m.current.nodes, id)
	}

	// 5. 节点都处理完，再删不用的出口
	for id := range m.current.exits {
		if wantExits[id] {
			continue
		}
		if err := m.core.RemoveOutbound(core.ExitTag(id)); err != nil {
			m.log.Warn("删除落地出口失败", "exit", id, "err", err)
		}
		delete(m.current.exits, id)
	}

	// 6. 自建落地
	if err := m.applyLanding(ds.GetLanding()); err != nil {
		fail(agentv1.ApplyFailure_ITEM_LANDING, 0, err)
	}

	// 7. 端口跳跃：按节点实际在跑的端口整体重建 nftables 规则
	failures = append(failures, m.applyPortHopping(ds)...)

	slices.SortFunc(failures, func(a, b *agentv1.ApplyFailure) int {
		return cmp.Or(cmp.Compare(a.GetItem(), b.GetItem()), cmp.Compare(a.GetId(), b.GetId()))
	})
	return failures
}

// usersOf 找出节点上放行的用户和他们的用户名。
func (m *Manager) usersOf(n *agentv1.Node, users map[uint64]*agentv1.User) ([]*agentv1.User, []string) {
	list := make([]*agentv1.User, 0, len(n.GetUserIds()))
	names := make([]string, 0, len(n.GetUserIds())) // 不能是 nil：nil 表示节点已删除
	for _, id := range n.GetUserIds() {
		u, ok := users[id]
		if !ok {
			m.log.Warn("节点引用了期望状态里没有的用户，跳过", "node", n.GetId(), "user", id)
			continue
		}
		list = append(list, u)
		names = append(names, core.UserName(id))
	}
	return list, names
}

func (m *Manager) applyExit(e *agentv1.Exit) error {
	fp := fingerprint(e)
	if old, ok := m.current.exits[e.GetId()]; ok && old == fp {
		return nil
	}
	outboundType, options, err := core.ExitOptions(e)
	if err != nil {
		return err
	}
	if err := m.core.SetOutbound(core.ExitTag(e.GetId()), outboundType, options); err != nil {
		return err
	}
	m.current.exits[e.GetId()] = fp
	return nil
}

// applyCertificate 校验期望状态里的证书，通过才换上；不通过时保持原来的证书。
// 期望状态里没有证书时（没有 TLS 节点就用不到）也保持原来的。
func (m *Manager) applyCertificate(cert *agentv1.Certificate) error {
	if cert.GetCertPem() == "" && cert.GetKeyPem() == "" {
		return nil
	}
	if err := core.CheckCertificate(cert); err != nil {
		return err
	}
	m.current.cert = cert
	return nil
}

func (m *Manager) applyNode(n *agentv1.Node, users []*agentv1.User, cert *agentv1.Certificate) error {
	id := n.GetId()
	tag := core.NodeTag(id)
	config := nodeConfig(n, cert)
	names := make([]string, 0, len(users))
	creds := make(map[string]string, len(users))
	for _, u := range users {
		name := core.UserName(u.GetId())
		names = append(names, name)
		creds[name] = fingerprint(u)
	}
	slices.Sort(names)

	old, exists := m.current.nodes[id]
	// 用户的凭据（UUID、密码、SS 密钥）也要比：重置凭据时用户名单不变，只有凭据变了
	if exists && old.config == config && maps.Equal(old.creds, creds) {
		return nil
	}
	inboundType, options, err := core.InboundOptions(n, users, cert)
	if err != nil {
		return err
	}
	record := appliedNode{
		port:    n.GetPort(),
		config:  config,
		users:   names,
		creds:   creds,
		inbound: core.InboundConfig{Type: inboundType, Options: options},
	}

	if exists && old.config == config {
		// 只有用户变了：增删用户，或者有用户的凭据变了
		switch n.GetProtocol().(type) {
		case *agentv1.Node_Shadowsocks2022:
			// Shadowsocks 能热更新用户（连同密钥），不用重建
			keys := make([]string, 0, len(users))
			userNames := make([]string, 0, len(users))
			for _, u := range users {
				userNames = append(userNames, core.UserName(u.GetId()))
				keys = append(keys, u.GetSsKey())
			}
			err := m.core.UpdateShadowsocksUsers(tag, userNames, keys)
			if err == nil {
				m.current.nodes[id] = record
				return nil
			}
			m.log.Warn("Shadowsocks 热更新用户失败，改为重建入站", "node", id, "err", err)
		case *agentv1.Node_Hysteria2:
			// 只删了用户：追踪层已经拦住他们，不重建。重建会断掉这个节点上所有人的会话（原型 V4）
			if !hasNewName(old.users, names) && !credsChanged(old.creds, creds) {
				record.inbound = old.inbound // 入站没动，还是原来的配置
				m.current.nodes[id] = record
				return nil
			}
		case *agentv1.Node_Mieru:
			// Mieru 能在运行中换用户列表，不用重建：重建会断掉这个节点上所有人的连接。
			// 有用户的密码变了时它会拒绝，改为重建，旧密码建立的底层连接才会断开
			err := m.core.UpdateMieruUsers(tag, record.inbound)
			if err == nil {
				m.current.nodes[id] = record
				return nil
			}
			if errors.Is(err, mieru.ErrPasswordChanged) {
				m.log.Info("Mieru 节点有用户的密码变了，重建", "node", id)
			} else {
				m.log.Warn("Mieru 热更新用户失败，改为重建", "node", id, "err", err)
			}
		}
	}

	if exists && old.port == n.GetPort() {
		// 端口没变：先校验，再删旧的建新的，失败时按原来的配置建回去
		err = m.core.RebuildInbound(tag, record.inbound, old.inbound)
	} else {
		// 新建，或者端口变了：新的起不来时旧的原样保留
		err = m.core.SetInbound(tag, record.inbound)
	}
	if err != nil {
		if errors.Is(err, core.ErrInboundStopped) {
			delete(m.current.nodes, id)
		}
		return err
	}
	m.current.nodes[id] = record
	return nil
}

func (m *Manager) applyLanding(l *agentv1.Landing) error {
	if l == nil {
		if m.current.landing != nil {
			if err := m.core.RemoveInbound(core.LandingTag); err != nil {
				m.log.Warn("删除自建落地入站失败", "err", err)
			}
			m.current.landing = nil
		}
		m.core.SetLandingSources(nil)
		return nil
	}

	// 放行的来源 IP 在分发出站里检查，随时可换，不用重建入站
	addrs := make([]netip.Addr, 0, len(l.GetAllowedSourceIps()))
	for _, s := range l.GetAllowedSourceIps() {
		addr, err := netip.ParseAddr(s)
		if err != nil {
			m.log.Warn("自建落地的来源 IP 格式不对，跳过", "ip", s)
			continue
		}
		addrs = append(addrs, addr)
	}
	m.core.SetLandingSources(addrs)

	withoutSources := proto.Clone(l).(*agentv1.Landing)
	withoutSources.AllowedSourceIps = nil
	fp := fingerprint(withoutSources)
	old := m.current.landing
	if old != nil && old.config == fp {
		return nil
	}
	inboundType, options, err := core.LandingOptions(l)
	if err != nil {
		return err
	}
	record := &appliedLanding{
		port:    l.GetPort(),
		config:  fp,
		inbound: core.InboundConfig{Type: inboundType, Options: options},
	}
	// 和节点一样：端口没变的先校验再重建，失败时按原来的配置建回去
	if old != nil && old.port == l.GetPort() {
		err = m.core.RebuildInbound(core.LandingTag, record.inbound, old.inbound)
	} else {
		err = m.core.SetInbound(core.LandingTag, record.inbound)
	}
	if err != nil {
		if errors.Is(err, core.ErrInboundStopped) {
			m.current.landing = nil
		}
		return err
	}
	m.current.landing = record
	return nil
}

// applyPortHopping 整体重建端口跳跃规则（启动后第一次应用时也会清掉上次留下的旧规则）。
// 规则转到节点实际在跑的端口。某个节点的范围不合法、盖住了别的节点的端口、和别的节点的范围重叠，
// 或者节点没在运行，只有它报失败，其他节点照常生效；提交失败时开了端口跳跃的节点都报失败，
// 原来的规则保持不变（提交是原子的）。
func (m *Manager) applyPortHopping(ds *agentv1.DesiredState) []*agentv1.ApplyFailure {
	var failures []*agentv1.ApplyFailure
	fail := func(id uint64, err error) {
		failures = append(failures, &agentv1.ApplyFailure{Item: agentv1.ApplyFailure_ITEM_PORT_HOPPING, Id: id, Reason: err.Error()})
	}

	nodes := slices.Clone(ds.GetNodes())
	slices.SortFunc(nodes, func(a, b *agentv1.Node) int { return cmp.Compare(a.GetId(), b.GetId()) })
	var rules []firewall.Rule
	for _, n := range nodes {
		hop := n.GetHysteria2().GetPortHopping()
		if hop == nil {
			continue
		}
		start, end := hop.GetStart(), hop.GetEnd()
		if start == 0 || end > 65535 || start > end {
			fail(n.GetId(), fmt.Errorf("端口跳跃范围 %d-%d 不合法", start, end))
			continue
		}
		running, ok := m.current.nodes[n.GetId()]
		if !ok || running.inbound.Type != C.TypeHysteria2 {
			fail(n.GetId(), errors.New("这个节点的 Hysteria2 入站没有在运行，端口跳跃没有目标"))
			continue
		}
		if err := hoppingConflict(n, nodes, rules, start, end); err != nil {
			fail(n.GetId(), err)
			continue
		}
		rules = append(rules, firewall.Rule{NodeID: n.GetId(), Start: uint16(start), End: uint16(end), Port: uint16(running.port)})
	}

	if err := m.firewall.Apply(rules); err != nil {
		for _, r := range rules {
			fail(r.NodeID, err)
		}
		if msg := err.Error(); len(rules) == 0 && msg != m.lastFirewallErr {
			// 没有节点开端口跳跃时只是清理，失败了不报给主控；同样的错误只记一次
			m.log.Warn("清理端口跳跃规则失败", "err", err)
			m.lastFirewallErr = msg
		}
		return failures
	}
	m.lastFirewallErr = ""
	return failures
}

// hoppingConflict 检查节点 n 的端口跳跃范围：不能盖住别的节点的端口，不能和已经接受的范围重叠。
func hoppingConflict(n *agentv1.Node, nodes []*agentv1.Node, accepted []firewall.Rule, start, end uint32) error {
	for _, other := range nodes {
		if other.GetId() != n.GetId() && other.GetPort() >= start && other.GetPort() <= end {
			return fmt.Errorf("端口跳跃范围 %d-%d 盖住了节点 %d 的端口 %d", start, end, other.GetId(), other.GetPort())
		}
	}
	for _, r := range accepted {
		if start <= uint32(r.End) && uint32(r.Start) <= end {
			return fmt.Errorf("端口跳跃范围 %d-%d 和节点 %d 的 %d-%d 重叠", start, end, r.NodeID, r.Start, r.End)
		}
	}
	return nil
}

// nodeConfig 是节点「除用户以外」的配置指纹：它变了才需要重建入站。
// 落地出口由分发出站按表选，端口跳跃由防火墙负责，都不影响入站本身，不算在内；
// TLS 节点的证书变了要重建，算在内。
func nodeConfig(n *agentv1.Node, cert *agentv1.Certificate) string {
	c := proto.Clone(n).(*agentv1.Node)
	c.UserIds = nil
	c.ExitId = 0
	if h := c.GetHysteria2(); h != nil {
		h.PortHopping = nil
	}
	switch n.GetProtocol().(type) {
	case *agentv1.Node_Hysteria2, *agentv1.Node_Anytls:
		return fingerprint(c) + fingerprint(cert)
	}
	return fingerprint(c)
}

func fingerprint(msg proto.Message) string {
	data, err := proto.MarshalOptions{Deterministic: true}.Marshal(msg)
	if err != nil {
		// 只有消息本身不合法才会失败；返回空串会被当成「变了」，照常重建
		return ""
	}
	sum := sha256.Sum256(data)
	return hex.EncodeToString(sum[:])
}

// hasNewName 判断 names 里有没有 old 里没有的用户。两个列表都已排序。
func hasNewName(old, names []string) bool {
	for _, n := range names {
		if _, found := slices.BinarySearch(old, n); !found {
			return true
		}
	}
	return false
}

// credsChanged 判断两边都有的用户里，有没有人的凭据变了。
func credsChanged(old, next map[string]string) bool {
	for name, fp := range next {
		if prev, ok := old[name]; ok && prev != fp {
			return true
		}
	}
	return false
}

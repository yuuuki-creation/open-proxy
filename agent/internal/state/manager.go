// Package state 顺序处理主控下发的期望状态：存盘、逐项应用到 sing-box、Mieru 和 nftables、上报结果。
// 只有这里的 goroutine 改它们，实现 architecture.md「一把锁串行化所有变更」。
// 应用规则见 protocol.md「应用规则」。
package state

import (
	"context"
	"log/slog"
	"sync/atomic"
	"time"

	"github.com/yuuuki-creation/open-proxy/agent/internal/core"
	"github.com/yuuuki-creation/open-proxy/agent/internal/firewall"
	agentv1 "github.com/yuuuki-creation/open-proxy/agent/internal/pb/openproxy/agent/v1"
	"github.com/yuuuki-creation/open-proxy/agent/internal/tracker"
)

// Reporter 把应用结果发给主控。没连上时由实现方记下，连上后再发。
type Reporter func(*agentv1.StateReport)

// Manager 持有当前生效的期望状态。
type Manager struct {
	core     *core.Core
	tracker  *tracker.Tracker
	firewall *firewall.Firewall
	store    *Store
	report   Reporter
	log      *slog.Logger
	incoming chan *agentv1.DesiredState

	appliedVersion atomic.Uint64
	// 下面的只在 Run 的 goroutine 里读写
	current         applied
	lastFirewallErr string // 清理端口跳跃规则失败的原因，同样的错误只记一次日志
}

// applied 记录每一项当前生效的配置，下一份期望状态来了按它算差异。
type applied struct {
	nodes   map[uint64]appliedNode
	exits   map[uint64]string    // 出口 ID -> 配置指纹
	landing *appliedLanding      // 自建落地；nil 表示没有
	cert    *agentv1.Certificate // 校验通过的最近一份证书，TLS 节点用它；nil 表示还没有
}

type appliedNode struct {
	port    uint32
	config  string             // 除用户以外的配置指纹，TLS 节点含证书
	users   []string           // 排好序的用户名
	creds   map[string]string  // 用户名 -> 凭据指纹（整条 User：UUID、密码、SS 密钥）
	inbound core.InboundConfig // 正在运行的入站配置，端口没变的重建失败时按它建回去
}

type appliedLanding struct {
	port    uint32
	config  string             // 除来源 IP 以外的配置指纹
	inbound core.InboundConfig // 同 appliedNode.inbound
}

func NewManager(c *core.Core, t *tracker.Tracker, fw *firewall.Firewall, store *Store, report Reporter) *Manager {
	return &Manager{
		core:     c,
		tracker:  t,
		firewall: fw,
		store:    store,
		report:   report,
		log:      slog.With("module", "state"),
		incoming: make(chan *agentv1.DesiredState, 1),
		current: applied{
			nodes: make(map[uint64]appliedNode),
			exits: make(map[uint64]string),
		},
	}
}

// Submit 交一份新的期望状态，不阻塞。还没来得及处理的旧状态直接被替换：只有最新的一份有意义。
// 只能从一个 goroutine 调用（收主控消息的那个）。
func (m *Manager) Submit(ds *agentv1.DesiredState) {
	for {
		select {
		case m.incoming <- ds:
			return
		default:
		}
		select {
		case <-m.incoming:
		default:
		}
	}
}

// AppliedVersion 返回已经处理完的期望状态版本，Hello 里上报。
func (m *Manager) AppliedVersion() uint64 {
	return m.appliedVersion.Load()
}

// Run 顺序处理期望状态，直到 ctx 取消。
func (m *Manager) Run(ctx context.Context) {
	for {
		select {
		case <-ctx.Done():
			return
		case ds := <-m.incoming:
			m.handle(ds)
		}
	}
}

func (m *Manager) handle(ds *agentv1.DesiredState) {
	// 先存盘：应用到一半进程退出，重启后按最新的状态重来
	if err := m.store.Save(ds); err != nil {
		m.log.Error("保存期望状态失败，照常应用", "err", err)
	}

	start := time.Now()
	failures := m.apply(ds)
	m.appliedVersion.Store(ds.GetVersion())

	m.log.Info("期望状态已应用", "version", ds.GetVersion(), "failures", len(failures), "elapsed", time.Since(start).Round(time.Millisecond))
	for _, f := range failures {
		m.log.Warn("有一项没应用成功", "item", f.GetItem(), "id", f.GetId(), "reason", f.GetReason())
	}
	m.report(&agentv1.StateReport{AppliedVersion: ds.GetVersion(), Failures: failures})
}

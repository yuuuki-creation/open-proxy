package main

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"sync"
	"sync/atomic"
	"time"

	"github.com/yuuuki-creation/open-proxy/agent/internal/conn"
	agentv1 "github.com/yuuuki-creation/open-proxy/agent/internal/pb/openproxy/agent/v1"
	"github.com/yuuuki-creation/open-proxy/agent/internal/reality"
	"github.com/yuuuki-creation/open-proxy/agent/internal/state"
	"github.com/yuuuki-creation/open-proxy/agent/internal/upgrade"
)

// agentHandler 把主控发来的消息分给各模块，实现 conn.Handler。
type agentHandler struct {
	state     *state.Manager
	reporter  *stateReporter
	traffic   *trafficReporter // 在 client.Run 之前设置
	upgrader  *upgrade.Upgrader
	masterURL string
	token     string
	exit      context.CancelCauseFunc

	// 启动时有升级标记（新版本试运行，或者上次升级没成功），通过认证后删掉；只在收消息的 goroutine 里读写
	bootPending   bool
	authenticated chan struct{} // 第一次通过认证时关闭
	authOnce      sync.Once

	mu             sync.Mutex
	rolledBackFrom string           // Hello 里报的「上次升级回滚了」，主控知道后清空
	after          map[uint64]error // 请求 ID -> 回复发出后要让 Agent 退出的原因

	scanning atomic.Bool // 正在扫描 REALITY 伪装目标，同一时间只做一个
}

var _ conn.Handler = (*agentHandler)(nil)

func (h *agentHandler) HelloState() (uint64, string) {
	h.mu.Lock()
	defer h.mu.Unlock()
	return h.state.AppliedVersion(), h.rolledBackFrom
}

func (h *agentHandler) OnConnected(sync bool) {
	h.confirmBoot()
	if !sync {
		slog.Warn("和主控版本不一致，暂停同步期望状态，等管理员在面板上升级 Agent")
		return
	}
	// protocol.md：连接成功、同步开始时也发一次状态上报
	h.reporter.resend()
}

// confirmBoot 在通过认证后调用：新版本试运行成功，或者「上次升级回滚了」已经报给主控，删掉升级标记。
func (h *agentHandler) confirmBoot() {
	if h.bootPending {
		h.bootPending = false
		if err := h.upgrader.Confirm(); err != nil {
			slog.Error("删除升级标记失败", "err", err)
		} else {
			slog.Info("已通过主控认证，删除升级标记")
		}
		h.mu.Lock()
		h.rolledBackFrom = ""
		h.mu.Unlock()
	}
	h.authOnce.Do(func() { close(h.authenticated) })
}

func (h *agentHandler) OnDisconnected() {}

func (h *agentHandler) OnPush(msg *agentv1.MasterMessage) {
	switch body := msg.GetBody().(type) {
	case *agentv1.MasterMessage_DesiredState:
		h.state.Submit(body.DesiredState)
	default:
		slog.Debug("收到不处理的推送，忽略", "id", msg.GetId())
	}
}

func (h *agentHandler) OnRequest(ctx context.Context, msg *agentv1.MasterMessage) (*agentv1.AgentMessage, error) {
	switch body := msg.GetBody().(type) {
	case *agentv1.MasterMessage_Upgrade:
		return h.upgrade(ctx, msg.GetId(), body.Upgrade), nil
	case *agentv1.MasterMessage_Uninstall:
		return h.uninstall(msg.GetId()), nil
	case *agentv1.MasterMessage_CheckRealityTargets:
		return h.checkReality(ctx, body.CheckRealityTargets)
	case *agentv1.MasterMessage_ScanRealityTargets:
		return h.scanReality(ctx, body.ScanRealityTargets)
	}
	return nil, conn.ErrUnsupported
}

// checkReality 检测管理员选中的 REALITY 伪装目标；单个目标的问题写在它的结果里。
func (h *agentHandler) checkReality(ctx context.Context, req *agentv1.CheckRealityTargets) (*agentv1.AgentMessage, error) {
	slog.Info("检测 REALITY 伪装目标", "targets", req.GetTargets())
	results, err := reality.Check(ctx, req.GetTargets())
	if err != nil {
		return nil, err
	}
	return &agentv1.AgentMessage{Body: &agentv1.AgentMessage_CheckRealityTargetsResult{
		CheckRealityTargetsResult: &agentv1.CheckRealityTargetsResult{Results: results},
	}}, nil
}

// scanReality 扫描网段里的 REALITY 伪装目标，结束时一次性回复；整体失败时回 ErrorReply（处理失败）。
func (h *agentHandler) scanReality(ctx context.Context, req *agentv1.ScanRealityTargets) (*agentv1.AgentMessage, error) {
	if !h.scanning.CompareAndSwap(false, true) {
		return nil, errors.New("已经有一个扫描在进行，等它结束再发")
	}
	defer h.scanning.Store(false)
	slog.Info("开始扫描 REALITY 伪装目标", "cidr", req.GetCidr(), "concurrency", req.GetConcurrency(), "max_per_second", req.GetMaxPerSecond())
	start := time.Now()
	candidates, err := reality.Scan(ctx, req)
	if err != nil {
		slog.Warn("扫描 REALITY 伪装目标失败", "cidr", req.GetCidr(), "err", err)
		return nil, fmt.Errorf("扫描: %w", err)
	}
	slog.Info("扫描 REALITY 伪装目标完成", "cidr", req.GetCidr(), "candidates", len(candidates), "elapsed", time.Since(start).Round(time.Second))
	return &agentv1.AgentMessage{Body: &agentv1.AgentMessage_ScanRealityTargetsResult{
		ScanRealityTargetsResult: &agentv1.ScanRealityTargetsResult{Candidates: candidates},
	}}, nil
}

// upgrade 处理 Upgrade：下载、校验、替换。成功时回复发出后补发流量上报并退出（AfterReply）。
func (h *agentHandler) upgrade(ctx context.Context, id uint64, req *agentv1.Upgrade) *agentv1.AgentMessage {
	reply := func(err error) *agentv1.AgentMessage {
		result := &agentv1.UpgradeResult{Ok: err == nil}
		if err != nil {
			result.Error = err.Error()
		}
		return &agentv1.AgentMessage{Body: &agentv1.AgentMessage_UpgradeResult{UpgradeResult: result}}
	}
	if err := h.upgrader.Begin("升级"); err != nil {
		return reply(err)
	}
	slog.Info("开始升级", "from", version, "to", req.GetVersion(), "path", req.GetDownloadPath())
	if err := h.upgrader.Upgrade(ctx, req, h.masterURL, h.token); err != nil {
		h.upgrader.End()
		slog.Error("升级失败，继续用现在的版本", "to", req.GetVersion(), "err", err)
		return reply(err)
	}
	// 不释放执行权：回复发出后就退出
	slog.Info("新版本已就位，回复主控后退出", "to", req.GetVersion())
	h.exitAfterReply(id, errUpgraded)
	return reply(nil)
}

// uninstall 处理 Uninstall：先回复「开始卸载」，回复发出后补发流量上报并退出，由 run 卸载（AfterReply）。
func (h *agentHandler) uninstall(id uint64) *agentv1.AgentMessage {
	result := &agentv1.UninstallResult{Ok: true}
	if err := h.upgrader.Begin("卸载"); err != nil {
		result = &agentv1.UninstallResult{Ok: false, Error: err.Error()}
	} else {
		slog.Warn("主控让 Agent 卸载自己（服务器已在面板上删除）")
		h.exitAfterReply(id, errUninstall)
	}
	return &agentv1.AgentMessage{Body: &agentv1.AgentMessage_UninstallResult{UninstallResult: result}}
}

func (h *agentHandler) exitAfterReply(id uint64, cause error) {
	h.mu.Lock()
	h.after[id] = cause
	h.mu.Unlock()
}

func (h *agentHandler) AfterReply(msg *agentv1.MasterMessage, sendErr error) {
	h.mu.Lock()
	cause, ok := h.after[msg.GetId()]
	delete(h.after, msg.GetId())
	h.mu.Unlock()
	if !ok {
		return
	}
	// 退出前补发一次流量上报（protocol.md「上报」）；回复没发出去（连接断了）也照样退出
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	h.traffic.send(ctx)
	cancel()
	h.exit(cause)
}

// stateReporter 把状态管理的应用结果发给主控。没连上时记下最近一份，连上后补发。
type stateReporter struct {
	mu     sync.Mutex
	client *conn.Client
	last   *agentv1.StateReport
}

func (r *stateReporter) setClient(c *conn.Client) {
	r.mu.Lock()
	r.client = c
	r.mu.Unlock()
}

// report 由状态管理在每次应用完后调用。
func (r *stateReporter) report(rep *agentv1.StateReport) {
	r.mu.Lock()
	r.last = rep
	c := r.client
	r.mu.Unlock()
	if c != nil {
		r.send(c, rep)
	}
}

// resend 补发最近一份报告。
func (r *stateReporter) resend() {
	r.mu.Lock()
	rep, c := r.last, r.client
	r.mu.Unlock()
	if rep != nil && c != nil {
		r.send(c, rep)
	}
}

func (r *stateReporter) send(c *conn.Client, rep *agentv1.StateReport) {
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	err := c.Send(ctx, &agentv1.AgentMessage{Body: &agentv1.AgentMessage_StateReport{StateReport: rep}})
	if err != nil && !errors.Is(err, conn.ErrNotConnected) && !errors.Is(err, conn.ErrNotSynced) {
		slog.Warn("发送状态上报失败，下次连上时重发", "err", err)
	}
}

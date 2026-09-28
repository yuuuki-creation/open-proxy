package main

import (
	"context"
	"errors"
	"log/slog"
	"sync"
	"time"

	"github.com/yuuuki-creation/open-proxy/agent/internal/conn"
	"github.com/yuuuki-creation/open-proxy/agent/internal/core"
	agentv1 "github.com/yuuuki-creation/open-proxy/agent/internal/pb/openproxy/agent/v1"
	"github.com/yuuuki-creation/open-proxy/agent/internal/sysinfo"
	"github.com/yuuuki-creation/open-proxy/agent/internal/tracker"
)

// trafficInterval 是流量上报的周期（protocol.md「上报」）。
const trafficInterval = 10 * time.Second

// trafficReporter 定时把追踪层的累计流量和网卡计数发给主控。
// 计数都是从进程启动算起的累计值，没连上时跳过这一次，下一次上报自然带上。
type trafficReporter struct {
	client     *conn.Client
	tracker    *tracker.Tracker
	instanceID uint64

	mu         sync.Mutex // 定时上报和升级、卸载前的补发可能同时发生，一次只发一份
	lastNICErr string     // 只在网卡读取出错的原因变化时打日志，避免每 10 秒刷一次
}

func (r *trafficReporter) run(ctx context.Context) {
	ticker := time.NewTicker(trafficInterval)
	defer ticker.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
			r.send(ctx)
		}
	}
}

// send 立即发一次流量上报。升级、卸载退出前也补发一次（handler.go 的 AfterReply）。
func (r *trafficReporter) send(ctx context.Context) {
	r.mu.Lock()
	defer r.mu.Unlock()
	report := r.build()
	sendCtx, cancel := context.WithTimeout(ctx, 10*time.Second)
	defer cancel()
	err := r.client.Send(sendCtx, &agentv1.AgentMessage{Body: &agentv1.AgentMessage_TrafficReport{TrafficReport: report}})
	if err != nil && !errors.Is(err, conn.ErrNotConnected) {
		slog.Warn("发送流量上报失败", "err", err)
	}
}

func (r *trafficReporter) build() *agentv1.TrafficReport {
	report := &agentv1.TrafficReport{InstanceId: r.instanceID}
	for _, s := range r.tracker.Snapshot() {
		// 只发非零的行
		if s.Uplink <= 0 && s.Downlink <= 0 {
			continue
		}
		nodeID, ok := core.ParseNodeTag(s.Inbound)
		if !ok {
			continue
		}
		userID, ok := core.ParseUserName(s.User)
		if !ok {
			continue
		}
		report.Users = append(report.Users, &agentv1.UserTraffic{
			NodeId:   nodeID,
			UserId:   userID,
			Uplink:   uint64(max(s.Uplink, 0)),
			Downlink: uint64(max(s.Downlink, 0)),
		})
	}

	nic, err := sysinfo.ReadNetwork()
	if err != nil {
		if msg := err.Error(); msg != r.lastNICErr {
			slog.Warn("读取网卡流量失败，这次上报不带网卡计数", "err", err)
			r.lastNICErr = msg
		}
		return report
	}
	r.lastNICErr = ""
	report.Network = &agentv1.NetworkTraffic{
		BootId:    nic.BootID,
		Interface: nic.Interface,
		RxBytes:   nic.RxBytes,
		TxBytes:   nic.TxBytes,
	}
	return report
}

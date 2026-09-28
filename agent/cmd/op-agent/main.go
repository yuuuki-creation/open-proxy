// op-agent 是 open-proxy 的 Agent：连上主控，按期望状态运行 sing-box 和 Mieru，上报流量。
// 设计见 main 分支的 docs/design-docs/architecture.md「Agent 内部」和 protocol.md。
package main

import (
	"context"
	"crypto/rand"
	"encoding/binary"
	"errors"
	"flag"
	"fmt"
	"log/slog"
	"os"
	"os/signal"
	"path/filepath"
	"runtime"
	"sync"
	"syscall"
	"time"

	"github.com/yuuuki-creation/open-proxy/agent/internal/conn"
	"github.com/yuuuki-creation/open-proxy/agent/internal/core"
	"github.com/yuuuki-creation/open-proxy/agent/internal/firewall"
	agentv1 "github.com/yuuuki-creation/open-proxy/agent/internal/pb/openproxy/agent/v1"
	"github.com/yuuuki-creation/open-proxy/agent/internal/state"
	"github.com/yuuuki-creation/open-proxy/agent/internal/tracker"
	"github.com/yuuuki-creation/open-proxy/agent/internal/upgrade"
)

// 编译时注入：-ldflags "-X main.version=0.1.0 -X main.upgradePublicKey=<base64 公钥>"。
var (
	// 和主控版本一致才同步期望状态，见 protocol.md「兼容规则」
	version = "dev"
	// 验证升级包签名的 Ed25519 公钥（标准 base64）；为空时不允许升级
	upgradePublicKey = ""
)

const (
	defaultConfigPath = "/etc/op-agent/op-agent.conf"
	defaultDataDir    = "/var/lib/op-agent"
)

// 让 Agent 退出的原因，由处理主控消息的代码触发，run 据此决定退出前做什么。
var (
	errUpgraded   = errors.New("新版本已就位，退出后由 systemd 拉起新版本")
	errRolledBack = errors.New("新版本试运行没通过，已换回旧版本，退出后由 systemd 拉起旧版本")
	errUninstall  = errors.New("主控让 Agent 卸载自己")
)

// options 是命令行参数。除了 -version，正式安装时都用默认值，测试时才改。
type options struct {
	configPath string
	dataDir    string
	nftTable   string
}

func main() {
	var opts options
	flag.StringVar(&opts.configPath, "config", defaultConfigPath, "配置文件路径")
	flag.StringVar(&opts.dataDir, "data-dir", defaultDataDir, "数据目录：本地保存的期望状态、升级标记")
	flag.StringVar(&opts.nftTable, "nft-table", firewall.DefaultTable, "端口跳跃用的 nftables 表名（inet 族），在测试机上改成测试专用的表")
	showVersion := flag.Bool("version", false, "打印版本号后退出")
	flag.Parse()

	if *showVersion {
		fmt.Println(version)
		return
	}

	// 输出到 stderr，由 systemd 收进 journald
	slog.SetDefault(slog.New(slog.NewTextHandler(os.Stderr, nil)))

	if err := run(opts); err != nil {
		slog.Error("退出", "err", err)
		os.Exit(1)
	}
}

func run(opts options) error {
	binaryPath, err := os.Executable()
	if err != nil {
		return fmt.Errorf("找不到自己的二进制文件: %w", err)
	}
	// 先看升级标记，再做别的：新版本哪怕读配置就出错，也要先把启动次数记下来，启动太多次就换回旧版本
	up := upgrade.New(upgrade.Config{Binary: binaryPath, DataDir: opts.dataDir, Version: version, PublicKey: upgradePublicKey})
	boot, err := up.Startup()
	if err != nil {
		slog.Error("读升级标记失败，当作没有升级", "err", err)
	}
	if boot.RollbackNow {
		slog.Error("新版本试运行期间启动了太多次，换回旧版本")
		if err := up.Rollback(); err != nil {
			slog.Error("换回旧版本失败，继续用新版本", "err", err)
			up.Confirm()
			boot.Trial = false
		} else {
			return nil // 由 systemd 拉起旧版本
		}
	}

	cfg, err := loadConfig(opts.configPath)
	if err != nil {
		return err
	}
	instanceID, err := randomInstanceID()
	if err != nil {
		return err
	}
	slog.Info("启动", "version", version, "master", cfg.MasterURL, "token", redact(cfg.Token), "instance_id", instanceID,
		"trial", boot.Trial, "rolled_back_from", boot.RolledBackFrom)

	// 收到 SIGTERM 等信号时 ctx 取消；处理主控消息的代码调 exit 让 Agent 退出（升级、回滚、卸载）
	ctx, stop := signal.NotifyContext(context.Background(), syscall.SIGINT, syscall.SIGTERM)
	defer stop()
	appCtx, exit := context.WithCancelCause(ctx)
	defer exit(nil)

	// 先按本地保存的期望状态启动 sing-box，主控连不上也能继续服务
	tr := tracker.New()
	singbox, err := core.Start(ctx, tr)
	if err != nil {
		return err
	}
	reporter := &stateReporter{}
	fw := firewall.New(opts.nftTable)
	store := state.NewStore(filepath.Join(opts.dataDir, "state.pb"))
	manager := state.NewManager(singbox, tr, fw, store, reporter.report)
	local, err := store.Load()
	if err != nil {
		slog.Error("本地保存的期望状态读不出来，等主控下发", "err", err)
	}
	if local != nil {
		slog.Info("按本地保存的期望状态启动", "version", local.GetVersion())
		manager.Submit(local)
	} else if err := fw.Remove(); err != nil {
		// 有本地状态时，第一次应用会整体重建端口跳跃规则；没有时清掉以前留下的规则
		slog.Warn("清理以前留下的端口跳跃规则失败", "table", fw.Table(), "err", err)
	}
	// 状态管理的 goroutine 是唯一改 sing-box、Mieru 和 nftables 的地方：退出时先等它停下，再关 sing-box
	managerCtx, stopManager := context.WithCancel(ctx)
	managerDone := make(chan struct{})
	go func() {
		defer close(managerDone)
		manager.Run(managerCtx)
	}()
	var shutdownOnce sync.Once
	shutdown := func() {
		shutdownOnce.Do(func() {
			stopManager()
			<-managerDone
			singbox.Close()
		})
	}
	defer shutdown()

	handler := &agentHandler{
		state:          manager,
		reporter:       reporter,
		upgrader:       up,
		masterURL:      cfg.MasterURL,
		token:          cfg.Token,
		exit:           exit,
		bootPending:    boot.Trial || boot.RolledBackFrom != "",
		rolledBackFrom: boot.RolledBackFrom,
		authenticated:  make(chan struct{}),
		after:          make(map[uint64]error),
	}
	client, err := conn.New(conn.Config{
		MasterURL:  cfg.MasterURL,
		Token:      cfg.Token,
		Version:    version,
		Arch:       currentArch(),
		InstanceID: instanceID,
	}, handler)
	if err != nil {
		return err
	}
	reporter.setClient(client)
	traffic := &trafficReporter{client: client, tracker: tr, instanceID: instanceID}
	handler.traffic = traffic
	go traffic.run(ctx)
	if boot.Trial {
		go watchTrial(appCtx, handler.authenticated, up, exit)
	}

	err = client.Run(appCtx)
	cause := context.Cause(appCtx)
	switch {
	case errors.Is(err, conn.ErrServerDeleted):
		slog.Warn("主控说这台服务器已在面板上删除，卸载 Agent")
		return uninstall(ctx, shutdown, fw, upgrade.Paths{Binary: binaryPath, Config: opts.configPath, DataDir: opts.dataDir})
	case errors.Is(cause, errUninstall):
		return uninstall(ctx, shutdown, fw, upgrade.Paths{Binary: binaryPath, Config: opts.configPath, DataDir: opts.dataDir})
	case errors.Is(cause, errUpgraded), errors.Is(cause, errRolledBack):
		slog.Info(cause.Error())
		return nil
	case err != nil:
		return err
	}
	slog.Info("收到退出信号，停止")
	return nil
}

// watchTrial 看着刚升级上来的新版本：TrialTimeout 内没有通过主控认证，就换回旧版本并退出。
func watchTrial(ctx context.Context, authenticated <-chan struct{}, up *upgrade.Upgrader, exit context.CancelCauseFunc) {
	timer := time.NewTimer(upgrade.TrialTimeout)
	defer timer.Stop()
	select {
	case <-authenticated:
	case <-ctx.Done():
	case <-timer.C:
		slog.Error("新版本在限定时间内没有通过主控认证，换回旧版本", "timeout", upgrade.TrialTimeout)
		if err := up.Rollback(); err != nil {
			slog.Error("换回旧版本失败，继续用新版本", "err", err)
			up.Confirm()
			return
		}
		exit(errRolledBack)
	}
}

// uninstall 卸载 Agent（protocol.md「删除服务器」）：停掉入站、删 nftables 表、删文件，
// 再让 systemd 停掉并删掉服务，然后等着被停掉。
func uninstall(sigCtx context.Context, shutdown func(), fw *firewall.Firewall, paths upgrade.Paths) error {
	shutdown()
	if err := fw.Remove(); err != nil {
		slog.Error("删除 nftables 表失败", "table", fw.Table(), "err", err)
	}
	stopping, err := upgrade.Uninstall(paths)
	if err != nil {
		slog.Error("卸载时有文件没删掉", "err", err)
	}
	if !stopping {
		slog.Info("卸载完成；不在 systemd 服务里运行，直接退出")
		return nil
	}
	slog.Info("卸载完成，等 systemd 停止服务")
	select {
	case <-sigCtx.Done():
	case <-time.After(time.Minute):
		slog.Warn("等了 1 分钟 systemd 还没停止服务，自己退出")
	}
	return nil
}

// randomInstanceID 生成本次启动的实例 ID，主控据此识别 Agent 重启过（计数从零开始）。
func randomInstanceID() (uint64, error) {
	var b [8]byte
	if _, err := rand.Read(b[:]); err != nil {
		return 0, fmt.Errorf("生成实例 ID: %w", err)
	}
	return binary.BigEndian.Uint64(b[:]) | 1, nil // 保证不是 0
}

// currentArch 返回编译时的 CPU 架构，主控据此选择升级用的二进制。
func currentArch() agentv1.Arch {
	switch runtime.GOARCH {
	case "amd64":
		return agentv1.Arch_ARCH_AMD64
	case "arm64":
		return agentv1.Arch_ARCH_ARM64
	}
	return agentv1.Arch_ARCH_UNSPECIFIED
}

// redact 隐去秘密，只留前 4 个字符，用于打日志。
func redact(secret string) string {
	const keep = 4
	if len(secret) <= keep {
		return "****"
	}
	return secret[:keep] + "****"
}

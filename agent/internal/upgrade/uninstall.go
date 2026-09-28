package upgrade

import (
	"bufio"
	"errors"
	"fmt"
	"io/fs"
	"log/slog"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"strings"
)

// Paths 是卸载要删的 Agent 文件（architecture.md「Agent 的文件」）。
type Paths struct {
	// 二进制；它的 .bak 备份和升级留下的临时文件一起删
	Binary string
	// 配置文件；所在目录空了也删
	Config string
	// 数据目录：只删 Agent 自己的文件，目录空了再删
	DataDir string
}

// Uninstall 删掉 Agent 的文件，再让 systemd 停掉并删掉自己的服务。调用前先停掉入站、删掉 nftables 表。
// 返回 true 表示已经让 systemd 停止本服务：调用方等着被停掉即可；返回 false 表示不在 systemd 服务里运行，
// 调用方自己退出。删文件出错不中断，都做完再一起返回错误。
func Uninstall(paths Paths) (stopping bool, err error) {
	var errs []error
	remove := func(path string) {
		if err := os.Remove(path); err != nil && !errors.Is(err, fs.ErrNotExist) {
			errs = append(errs, err)
		}
	}

	// 二进制：Linux 上运行中的程序可以删掉自己的文件
	remove(paths.Binary)
	remove(paths.Binary + ".bak")
	if tmps, err := filepath.Glob(filepath.Join(filepath.Dir(paths.Binary), ".op-agent-*.tmp")); err == nil {
		for _, tmp := range tmps {
			remove(tmp)
		}
	}
	// 配置文件和数据目录：只删自己的文件，目录空了才删，免得参数指错时误删别的东西
	remove(paths.Config)
	os.Remove(filepath.Dir(paths.Config))
	for _, pattern := range []string{"state.pb", markerName, ".state-*.tmp", ".upgrade-*.tmp"} {
		matches, _ := filepath.Glob(filepath.Join(paths.DataDir, pattern))
		for _, m := range matches {
			remove(m)
		}
	}
	os.Remove(paths.DataDir)

	unit := systemdUnit()
	if unit == "" {
		return false, errors.Join(errs...)
	}
	// 先取消开机启动、删掉服务文件（服务还在运行，systemd 会留着它直到停止）
	if out, err := exec.Command("systemctl", "disable", unit).CombinedOutput(); err != nil {
		// 临时单元（例如测试时 systemd-run 起的）不能 disable，不影响后面
		slog.Info("取消服务的开机启动没成功", "unit", unit, "err", err, "output", strings.TrimSpace(string(out)))
	}
	unitFile := filepath.Join("/etc/systemd/system", unit)
	if _, err := os.Stat(unitFile); err == nil {
		remove(unitFile)
		if out, err := exec.Command("systemctl", "daemon-reload").CombinedOutput(); err != nil {
			errs = append(errs, fmt.Errorf("systemctl daemon-reload: %w: %s", err, strings.TrimSpace(string(out))))
		}
	}
	// 让 systemd 停掉本服务。--no-block 只是把停止任务交给 systemd 就返回，
	// 停止由 systemd 自己执行，不会因为本进程被停掉而半途而废；服务是被明确停止的，Restart=always 也不会再拉起
	if out, err := exec.Command("systemctl", "stop", "--no-block", unit).CombinedOutput(); err != nil {
		var exitErr *exec.ExitError
		if errors.As(err, &exitErr) && !exitErr.Exited() {
			// systemctl 自己也在本服务里，停止任务开始后它和本进程一起收到了 SIGTERM：任务已经交给 systemd 了
			return true, errors.Join(errs...)
		}
		errs = append(errs, fmt.Errorf("systemctl stop %s: %w: %s", unit, err, strings.TrimSpace(string(out))))
		return false, errors.Join(errs...)
	}
	return true, errors.Join(errs...)
}

var unitName = regexp.MustCompile(`^[A-Za-z0-9@._-]+\.service$`)

// systemdUnit 从 /proc/self/cgroup 找本进程所在的 systemd 服务，例如 op-agent.service；不在服务里时返回空。
func systemdUnit() string {
	f, err := os.Open("/proc/self/cgroup")
	if err != nil {
		return ""
	}
	defer f.Close()
	scanner := bufio.NewScanner(f)
	for scanner.Scan() {
		// cgroup v2 只有一行：0::/system.slice/op-agent.service
		path, ok := strings.CutPrefix(scanner.Text(), "0::")
		if !ok {
			continue
		}
		unit := filepath.Base(path)
		if unitName.MatchString(unit) {
			return unit
		}
	}
	return ""
}

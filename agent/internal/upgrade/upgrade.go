// Package upgrade 负责 Agent 的升级、升级后的试运行和回滚，以及卸载。
// 设计见 main 分支 protocol.md「升级」「删除服务器」、architecture.md「发布、安装与升级」。
//
// 升级：下载新二进制（Token 放请求头）→ 校验 SHA-256 和 Ed25519 签名（对整个文件签名，公钥编译时内置）
// → 在旁边试跑 -version → 备份旧版本为 <二进制>.bak → 写升级标记 → 原子替换。调用方回复主控、补发流量上报后退出，
// 由 systemd 拉起新版本。新版本看到标记就是试运行：TrialTimeout 内通过主控认证就删标记，否则换回备份并退出。
package upgrade

import (
	"bytes"
	"context"
	"crypto/ed25519"
	"crypto/sha256"
	"debug/elf"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"io/fs"
	"net/http"
	"net/url"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"sync"
	"time"

	agentv1 "github.com/yuuuki-creation/open-proxy/agent/internal/pb/openproxy/agent/v1"
)

const (
	markerName = "upgrade.json"
	// TrialTimeout 是新版本启动后通过主控认证的时限，超时就换回旧版本。
	TrialTimeout = 3 * time.Minute
	// 新版本在试运行期间最多启动几次（崩溃后被 systemd 拉起也算），再多就直接换回旧版本
	maxTrialStarts = 3
	// 二进制大小的上限，防止把磁盘写满；现在的二进制约 55 MiB
	maxBinarySize = 256 << 20
	// 下载的时限；主控等 UpgradeResult 的时限是 5 分钟，留出校验和替换的时间
	downloadTimeout = 4 * time.Minute
)

// Config 是升级要用的信息。
type Config struct {
	// 正在运行的二进制的路径，替换和备份都在它旁边
	Binary string
	// 数据目录，升级标记放在这里
	DataDir string
	// 本版本号
	Version string
	// base64 编码的 Ed25519 公钥，编译时注入；为空时不允许升级
	PublicKey string
}

// Upgrader 负责升级、试运行和回滚。
type Upgrader struct {
	cfg    Config
	marker string

	mu   sync.Mutex
	busy string // 正在做的事（升级、卸载）；为空表示空闲
}

func New(cfg Config) *Upgrader {
	return &Upgrader{cfg: cfg, marker: filepath.Join(cfg.DataDir, markerName)}
}

// Begin 占住升级和卸载的执行权，同一时间只做一件；what 是要做的事，用于报错。
func (u *Upgrader) Begin(what string) error {
	u.mu.Lock()
	defer u.mu.Unlock()
	if u.busy != "" {
		return fmt.Errorf("正在%s，不能%s", u.busy, what)
	}
	u.busy = what
	return nil
}

// End 释放执行权。
func (u *Upgrader) End() {
	u.mu.Lock()
	u.busy = ""
	u.mu.Unlock()
}

// marker 是升级标记文件的内容（architecture.md「Agent 的文件」）。
type marker struct {
	From      string    `json:"from"`
	To        string    `json:"to"`
	StartedAt time.Time `json:"started_at"`
	// 新版本启动过几次
	Starts int `json:"starts"`
}

// Boot 是启动时看升级标记的结果。
type Boot struct {
	// 刚升级上来的新版本：要在 TrialTimeout 内通过主控认证，否则换回旧版本
	Trial bool
	// 新版本已经启动了太多次（多半是一启动就崩溃），应该直接换回旧版本
	RollbackNow bool
	// 上次升级没成功（回滚了，或者替换之前就中断了）时要升到的版本，Hello 里报给主控
	RolledBackFrom string
}

// Startup 在启动时调用，看升级标记。没有标记时返回零值。
func (u *Upgrader) Startup() (Boot, error) {
	m, err := u.readMarker()
	if err != nil {
		u.Confirm() // 读不出来的标记没有用了
		return Boot{}, err
	}
	if m == nil {
		return Boot{}, nil
	}
	if m.To != u.cfg.Version {
		// 升级没有生效：回滚过了，或者替换之前就中断了。认证成功、主控知道了以后再删标记
		return Boot{RolledBackFrom: m.To}, nil
	}
	m.Starts++
	if err := u.writeMarker(m); err != nil {
		return Boot{Trial: true}, fmt.Errorf("更新升级标记: %w", err)
	}
	return Boot{Trial: true, RollbackNow: m.Starts > maxTrialStarts}, nil
}

// Confirm 在通过主控认证后调用：试运行的新版本转正；回滚后的旧版本已经把回滚报给了主控。都删掉标记。
func (u *Upgrader) Confirm() error {
	if err := os.Remove(u.marker); err != nil && !errors.Is(err, fs.ErrNotExist) {
		return fmt.Errorf("删除升级标记: %w", err)
	}
	return nil
}

// Rollback 把备份换回来（标记留着，旧版本启动后据此报「上次升级回滚了」）。调用方随后退出，由 systemd 拉起旧版本。
func (u *Upgrader) Rollback() error {
	if err := os.Rename(u.cfg.Binary+".bak", u.cfg.Binary); err != nil {
		return fmt.Errorf("换回旧版本: %w", err)
	}
	syncDir(filepath.Dir(u.cfg.Binary))
	return nil
}

// Upgrade 下载、校验、替换。masterURL 是自己配置的主控地址（不带路径），下载地址由它和主控给的路径拼成，
// token 放在请求头里。成功后调用方回复主控、补发一次流量上报，再退出。
// 返回错误时什么都没变（旧二进制原样、没有标记）。
func (u *Upgrader) Upgrade(ctx context.Context, req *agentv1.Upgrade, masterURL, token string) error {
	pub, err := u.publicKey()
	if err != nil {
		return err
	}
	if req.GetVersion() == "" || req.GetVersion() == u.cfg.Version {
		return fmt.Errorf("目标版本 %q 不对（现在是 %s）", req.GetVersion(), u.cfg.Version)
	}
	if len(req.GetSha256()) != sha256.Size || len(req.GetSignature()) != ed25519.SignatureSize {
		return errors.New("SHA-256 或签名的长度不对")
	}
	link, err := downloadURL(masterURL, req.GetDownloadPath())
	if err != nil {
		return err
	}

	tmp, content, err := u.download(ctx, link, token)
	if err != nil {
		return err
	}
	defer os.Remove(tmp) // 改名成功后这里什么都不做
	if sum := sha256.Sum256(content); !bytes.Equal(sum[:], req.GetSha256()) {
		return errors.New("下载的文件和 SHA-256 对不上")
	}
	if !ed25519.Verify(pub, content, req.GetSignature()) {
		return errors.New("签名验证不通过")
	}
	if err := checkBinary(ctx, tmp, content, req.GetVersion()); err != nil {
		return err
	}

	// 备份旧版本：硬链接，不占空间；不行就复制
	bak := u.cfg.Binary + ".bak"
	if err := os.Remove(bak); err != nil && !errors.Is(err, fs.ErrNotExist) {
		return fmt.Errorf("删除旧的备份: %w", err)
	}
	if err := os.Link(u.cfg.Binary, bak); err != nil {
		if err := copyFile(u.cfg.Binary, bak); err != nil {
			return fmt.Errorf("备份旧版本: %w", err)
		}
	}
	if err := u.writeMarker(&marker{From: u.cfg.Version, To: req.GetVersion(), StartedAt: time.Now().UTC()}); err != nil {
		return fmt.Errorf("写升级标记: %w", err)
	}
	if err := os.Rename(tmp, u.cfg.Binary); err != nil {
		u.Confirm() // 没换成，标记也不要了
		return fmt.Errorf("替换二进制: %w", err)
	}
	syncDir(filepath.Dir(u.cfg.Binary))
	return nil
}

func (u *Upgrader) publicKey() (ed25519.PublicKey, error) {
	if u.cfg.PublicKey == "" {
		return nil, errors.New("这个 Agent 编译时没有内置升级验签公钥，不允许升级")
	}
	key, err := base64.StdEncoding.DecodeString(u.cfg.PublicKey)
	if err != nil || len(key) != ed25519.PublicKeySize {
		return nil, errors.New("内置的升级验签公钥格式不对，不允许升级")
	}
	return ed25519.PublicKey(key), nil
}

// downloadURL 用自己配置的主控地址拼出下载地址：主控只给路径，Token 就不会发到别的地方。
func downloadURL(masterURL, path string) (string, error) {
	if !strings.HasPrefix(path, "/") || strings.HasPrefix(path, "//") {
		return "", fmt.Errorf("下载路径 %q 不对，应该是以 / 开头的路径", path)
	}
	master, err := url.Parse(masterURL)
	if err != nil {
		return "", fmt.Errorf("主控地址: %w", err)
	}
	link, err := url.Parse(masterURL + path)
	if err != nil || link.Host != master.Host || link.Scheme != master.Scheme {
		return "", fmt.Errorf("下载路径 %q 不对", path)
	}
	return link.String(), nil
}

// download 把新二进制下载到旧二进制旁边的临时文件，返回临时文件路径和内容。
func (u *Upgrader) download(ctx context.Context, link, token string) (string, []byte, error) {
	ctx, cancel := context.WithTimeout(ctx, downloadTimeout)
	defer cancel()
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, link, nil)
	if err != nil {
		return "", nil, err
	}
	req.Header.Set("Authorization", "Bearer "+token)
	client := &http.Client{
		// 不跟随跳转：主控直接给文件；跟随跳转可能把请求（和 Token）带到别处
		CheckRedirect: func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse },
	}
	resp, err := client.Do(req)
	if err != nil {
		return "", nil, fmt.Errorf("下载新版本: %w", err)
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		return "", nil, fmt.Errorf("下载新版本: 主控回复 %s", resp.Status)
	}
	content, err := io.ReadAll(io.LimitReader(resp.Body, maxBinarySize+1))
	if err != nil {
		return "", nil, fmt.Errorf("下载新版本: %w", err)
	}
	if len(content) > maxBinarySize {
		return "", nil, fmt.Errorf("新版本超过 %d MiB，不接受", maxBinarySize>>20)
	}

	tmp, err := os.CreateTemp(filepath.Dir(u.cfg.Binary), ".op-agent-*.tmp")
	if err != nil {
		return "", nil, fmt.Errorf("创建临时文件: %w", err)
	}
	if err := writeAndSync(tmp, content, 0o755); err != nil {
		os.Remove(tmp.Name())
		return "", nil, fmt.Errorf("写临时文件: %w", err)
	}
	return tmp.Name(), content, nil
}

// checkBinary 确认新二进制是本机架构的 ELF，并且能跑起来、报的版本号就是要升的版本。
// 签名已经验过，这里是防止主控发错架构、或者新版本在这台机器上根本起不来（那样它也没法自己回滚）。
func checkBinary(ctx context.Context, path string, content []byte, want string) error {
	f, err := elf.NewFile(bytes.NewReader(content))
	if err != nil {
		return fmt.Errorf("新版本不是 ELF 可执行文件: %w", err)
	}
	machine := map[string]elf.Machine{"amd64": elf.EM_X86_64, "arm64": elf.EM_AARCH64}[runtime.GOARCH]
	if f.Machine != machine {
		return fmt.Errorf("新版本的架构是 %v，本机是 %s", f.Machine, runtime.GOARCH)
	}
	ctx, cancel := context.WithTimeout(ctx, 10*time.Second)
	defer cancel()
	out, err := exec.CommandContext(ctx, path, "-version").Output()
	if err != nil {
		return fmt.Errorf("试跑新版本: %w", err)
	}
	if got := strings.TrimSpace(string(out)); got != want {
		return fmt.Errorf("新版本自己报的版本号是 %q，不是 %q", got, want)
	}
	return nil
}

func (u *Upgrader) readMarker() (*marker, error) {
	data, err := os.ReadFile(u.marker)
	if errors.Is(err, fs.ErrNotExist) {
		return nil, nil
	}
	if err != nil {
		return nil, fmt.Errorf("读升级标记: %w", err)
	}
	m := &marker{}
	if err := json.Unmarshal(data, m); err != nil {
		return nil, fmt.Errorf("解析升级标记: %w", err)
	}
	return m, nil
}

// writeMarker 原子写入升级标记：先写临时文件、落盘，再改名覆盖。
func (u *Upgrader) writeMarker(m *marker) error {
	data, err := json.Marshal(m)
	if err != nil {
		return err
	}
	if err := os.MkdirAll(u.cfg.DataDir, 0o700); err != nil {
		return err
	}
	tmp, err := os.CreateTemp(u.cfg.DataDir, ".upgrade-*.tmp")
	if err != nil {
		return err
	}
	if err := writeAndSync(tmp, data, 0o600); err != nil {
		os.Remove(tmp.Name())
		return err
	}
	if err := os.Rename(tmp.Name(), u.marker); err != nil {
		os.Remove(tmp.Name())
		return err
	}
	syncDir(u.cfg.DataDir)
	return nil
}

// writeAndSync 写入、设权限、落盘并关闭文件。
func writeAndSync(f *os.File, data []byte, perm fs.FileMode) error {
	_, err := f.Write(data)
	if err == nil {
		err = f.Chmod(perm)
	}
	if err == nil {
		err = f.Sync()
	}
	if closeErr := f.Close(); err == nil {
		err = closeErr
	}
	return err
}

func copyFile(src, dst string) error {
	data, err := os.ReadFile(src)
	if err != nil {
		return err
	}
	f, err := os.OpenFile(dst, os.O_WRONLY|os.O_CREATE|os.O_TRUNC, 0o755)
	if err != nil {
		return err
	}
	return writeAndSync(f, data, 0o755)
}

// syncDir 把目录项的改动（改名、删除）落盘；失败只影响断电时的持久性，不报错。
func syncDir(dir string) {
	if d, err := os.Open(dir); err == nil {
		d.Sync()
		d.Close()
	}
}

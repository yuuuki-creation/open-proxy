# R20260927-main-01 VPS 重装后重做初始化

> 摘要：测试 VPS 被重装成 Debian 13，之前的配置和管理员的其他服务都没了。按 test-vps.md「一次性初始化」重做：slice（上限仍是 7 GiB）、目录、git、Go 1.26.5、新的只读 Deploy Key、仓库克隆和两个 worktree 全部就绪。go.dev 的 `.sha256` 地址已不能用来校验，改用下载列表 JSON。

- 状态：通过
- 日期：2026-09-27；执行：Windows 开发机 / Claude Code 会话
- 关联：无 ExecPlan；开发轮次 D20260927-main-01
- 代码：main `6fa5f7b`
- 环境：测试 VPS，Debian 13 (trixie) x86_64，内核 6.12，8 核，内存 11.68 GiB，磁盘 197 GB（初始化前已用 1.4 GB）

## 目的

VPS 重装后恢复到能编译和测试的状态，并把文档里过时的机器信息改掉。

## 做法

1. 只读检查：系统版本、内存、磁盘、cgroup、已装软件、监听端口、SSH 设置
2. 创建 `/etc/systemd/system/open-proxy.slice`，数值不变（`MemoryMax` 7 GiB、`MemoryHigh` 6.5 GiB、`MemorySwapMax` 0），`systemctl daemon-reload && systemctl start`
3. 建目录 `/opt/open-proxy/{toolchains,cache,src,bin,tools,runs}`
4. 装 git（apt）：系统里没有，克隆仓库要用
5. 装 Go：系统里没有了。从 go.dev 下载和开发机同版本的 `go1.26.5.linux-amd64.tar.gz`，按 go.dev 下载列表 JSON 里的 SHA-256 校验后解压到 `/opt/open-proxy/toolchains/go1.26.5/`
6. 仓库访问：生成 `/root/.ssh/open-proxy-deploy`，公钥用 `gh repo deploy-key add` 以只读 Deploy Key 加到仓库；`/root/.ssh/config` 配 `Host github-open-proxy`；GitHub 的主机公钥从 `gh api meta` 取，写进 VPS 的 `known_hosts`
7. `git clone` 到 `src/repo`，再为 panel、agent 建 detached worktree

## 结果

| 项 | 结果 |
| --- | --- |
| cgroup | `cgroup2fs` |
| slice 生效值 | `MemoryMax` 7168 MiB、`MemoryHigh` 6656 MiB、`MemorySwapMax` 0；`systemd-run --scope --slice=open-proxy.slice` 启动的进程落在 `/open.slice/open-proxy.slice/` 下（systemd 把名字里的「-」当层级） |
| git | 2.47.3 |
| Go | `go1.26.5 linux/amd64`；SHA-256 `5c2c3b16…81393f053`，go.dev 下载列表、dl.google.com 的 `.sha256`、下载的文件三者一致 |
| Deploy Key | 新增只读 key（ID 164574065）；重装前的旧 key（ID 163679303）还在 GitHub 上 |
| 仓库 | `src/repo` 在 main `6fa5f7b`；worktree `src/agent` = `e8df46b`、`src/panel` = `ec944f0` |
| 其他服务 | 没有：只有 sshd（22）和 systemd-resolved（53、5355）在监听；没有 Docker，也没有 `nft` 命令 |
| 资源 | 内存用了约 0.65 GiB；磁盘用了 1.8 GB，其中 `/opt/open-proxy` 272 MB |

## 结论

- 初始化完成，可以在 VPS 上编译和测试
- 重装后管理员的服务都不在了，内存上限仍保持 7 GiB，给他们以后部署的服务留余地
- `https://go.dev/dl/<文件名>.sha256` 现在返回 HTML 跳转页，不能用来校验；test-vps.md 已改为用下载列表 JSON
- 用 `ssh ... 'bash -s' <<EOF` 在 VPS 上执行脚本时，脚本里再调用的 ssh 会把后面的脚本当输入读走，要加 `</dev/null`
- SSH 仍允许 root 用密码登录（`PasswordAuthentication yes`），按规范不改，由管理员决定
- 旧 Deploy Key 的私钥已随重装丢失，删不删由管理员决定

## 下一步

其他开发电脑要重新把公钥加进 VPS、更新本机 `known_hosts`（新的 ED25519 指纹 `SHA256:<指纹>`）。

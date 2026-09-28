# 测试 VPS 使用规范

> 摘要：编译、静态检查和测试都在这台 VPS 上做（PR 上的 GitHub Actions 是合并前的最后检查）。**这台机器不是我们专用的**，管理员会在上面跑自己的服务（2026-09-27 重装后暂时没有），不能影响它们：我们启动的所有进程都必须跑在 `open-proxy.slice` 里（上限 7 GiB）。代码从 GitHub 拉取指定提交后编译，不在 VPS 上改代码；测试前加锁、结束清理；每轮写调试记录。

连接信息（IP、SSH 用户、系统）写在 `AGENTS.md` 的「测试服务器」一节，由管理员填写。所有开发电脑、所有会话都遵守同一套规则。

## 用途

- 只用于 open-proxy 的开发：原型验证、必须在 Linux 上做的调试、集成测试，以及这些测试需要的编译
- 不跑正式服务，不给朋友用，不存放真实用户数据和正式凭据

## 管理员自己的服务（重要）

VPS 不是我们专用的，管理员会在上面跑自己的服务。2026-09-27 重装后暂时没有：除了系统服务只有 SSH 在监听。重装前（2026-09-18）跑着服务 A（前端、服务端、MongoDB、Redis，占 8091）和 服务 B（Caddy 占 80、443），以后可能再部署。

规则（现在有没有别的服务都照此遵守）：

- 不碰 Docker、不停别人的容器、不动别人的防火墙规则（Docker 会在 `ip nat` 表里建自己的 DNAT 链）
- 不占用 80、443、8091；我们只用 20000–29999 和 30000–30999
- 内存上限 7 GiB（见下一节），给管理员的服务和系统留约 4.6 GiB
- 磁盘：总 197 GB，重装后已用约 1.8 GB，我们的编译缓存和产物注意别涨太快

## 连接

- 用 SSH 密钥登录。每台开发电脑自己配置密钥，私钥不进仓库；AI 不代为输入任何密码
- 每台开发电脑在 `~/.ssh/config` 里配置同一个别名，文档和命令里统一写 `ssh op-test`，不写死 IP：

```
Host op-test
    HostName <AGENTS.md 里的 IP>
    User <AGENTS.md 里的 SSH 用户>
    IdentityFile <本机私钥路径>
```

- 开始前先确认能连通：`ssh op-test 'uname -a'`
- VPS 重装后，每台开发电脑都要重新把自己的公钥加进 VPS 的 `/root/.ssh/authorized_keys`。主机密钥也会变，SSH 会拒绝连接：先在 VPS 上执行 `ssh-keygen -lf /etc/ssh/ssh_host_ed25519_key.pub` 看指纹，和本机收到的一致，再删掉本机 `known_hosts` 里的旧记录
- SSH 用户不是 root 时，`systemd-run`、nftables、写 `/etc` 等命令要加 `sudo`（需要管理员配置免密 sudo）

## 内存保护（必须遵守）

编译 Rust 很吃内存，内存耗尽会让整台 VPS 卡死，连 SSH 都进不去，还会拖垮机器上的其他服务。所以给我们的任务设硬上限：

- systemd slice `open-proxy.slice`：`MemoryMax=7168M`（7 GiB）、`MemoryHigh=6656M`（6.5 GiB，先限流）、`MemorySwapMax=0`。机器总内存 11.68 GiB，给管理员的服务和系统留约 4.6 GiB（重装前他们的服务约占 2.3 GiB）
- **我们启动的所有进程（编译、测试、原型、客户端）都必须跑在这个 slice 里**：
  - 前台命令（例如编译）：`systemd-run --scope --slice=open-proxy.slice -- <命令>`
  - 后台常驻：`systemd-run --unit=op-<run-id>-<名称> --slice=open-proxy.slice -- <命令>`
- 超过上限时，内核只会杀掉 slice 里的进程，系统和 SSH 不受影响
- 编译并行度：Rust 设 `CARGO_BUILD_JOBS=6`，Go 用 `go build -p 6`；被内存不足杀掉就调低，不许绕开 slice
- 不加 swap、不改内核内存参数；确实需要时先问管理员

## 一次性初始化

**最近一次在 2026-09-27 完成**（VPS 重装后重做），见调试记录 [R20260927-main-01](debug-runs/main/R20260927-main-01-vps-reinit.md)；第一次是 [R20260918-main-01](debug-runs/main/R20260918-main-01-vps-init.md)。VPS 再重装时照此执行：

1. 确认 cgroup v2：`stat -fc %T /sys/fs/cgroup` 输出 `cgroup2fs`
2. 创建 `/etc/systemd/system/open-proxy.slice`（内容见上一节的数值），然后 `systemctl daemon-reload && systemctl start open-proxy.slice`
3. 建目录：`/opt/open-proxy/{toolchains,cache,src,bin,tools,runs}`
4. 工具链：`apt-get install git`。Go 从 go.dev 下载和开发机同版本的 `linux-amd64` 包，按 go.dev 下载列表（`https://go.dev/dl/?mode=json&include=all`）里的 SHA-256 校验后，解压到 `toolchains/go<版本>/`（现在是 go1.26.5）。Rust 和 Node 等测试需要时再装到 `toolchains/`
   编译环境（2026-09-27 补装）：`apt-get install build-essential`（sqlite、ring 里有 C 代码）；rustup-init 按官方 `.sha256` 校验后装 Rust 1.96.0（`RUSTUP_HOME=toolchains/rustup`、`CARGO_HOME=cache/cargo`）；Node v24.14.1 按 `SHASUMS256.txt` 校验后装到 `toolchains/node-v24.14.1/`，再 `npm install -g pnpm@10`；这些环境变量都写在 `/opt/open-proxy/env.sh`
   测试用（2026-09-27 补装）：`apt-get install nftables`，只用 `nft` 命令读规则，服务没有启用（agent 分支 R20260927-agent-03）；发布用的交叉编译：zig 0.16.0 按 ziglang.org 的 `index.json` 校验后装到 `toolchains/zig-0.16.0/`，`cargo install --locked cargo-zigbuild@0.23.4`，`rustup target add x86_64-unknown-linux-musl aarch64-unknown-linux-musl`（panel 分支 R20260927-panel-04）；客户端 sing-box 1.14.1、mihomo v1.19.31 的官方发布包放在 `tools/`
5. 仓库访问：VPS 上生成 `/root/.ssh/open-proxy-deploy` 密钥，公钥作为**只读** Deploy Key 加到 GitHub 仓库；`/root/.ssh/config` 里配 `Host github-open-proxy`；GitHub 的主机公钥从 `gh api meta` 取，写进 VPS 的 `known_hosts`
6. 拉代码：`git clone github-open-proxy:yuuuki-creation/open-proxy.git /opt/open-proxy/src/repo`，再建两个 worktree：`git worktree add /opt/open-proxy/src/<分支> origin/<分支> --detach`

## 目录

| 路径 | 用途 |
| --- | --- |
| `/opt/open-proxy/toolchains/` | Go、Rust、Node 等工具链，目录名带版本 |
| `/opt/open-proxy/cache/` | 编译缓存：`GOMODCACHE`、`GOCACHE`、`CARGO_HOME`、npm 缓存 |
| `/opt/open-proxy/src/repo/` | 仓库克隆 |
| `/opt/open-proxy/src/panel/`、`src/agent/` | 两个分支的 worktree |
| `/opt/open-proxy/bin/` | 编译产物，文件名 `<名称>-<分支>-<短提交号>` |
| `/opt/open-proxy/tools/` | 测试用的客户端和工具（sing-box、mihomo 等），文件名带版本号 |
| `/opt/open-proxy/runs/<run-id>/` | 每轮调试的工作目录：运行时生成的配置、日志、输出。测试脚本不放这里，提交进仓库（见 [debug-runs/README.md](debug-runs/README.md)） |
| `/opt/open-proxy/LOCK` | 测试占用锁 |
| `/opt/open-proxy/env.sh` | 编译环境：工具链路径和缓存目录，编译前 `source` |

## 编译和测试的分工

| 在哪做 | 做什么 |
| --- | --- |
| 测试 VPS | 开发时的编译、静态检查、测试（按下一节的流程）；原型验证、集成测试 |
| GitHub Actions | 合并前的最后检查：开 PR 自动编译和静态检查，见 [git-workflow.md](git-workflow.md)「PR 与编译」 |
| 开发电脑 | 写代码；可以解析依赖（生成锁文件、下载依赖源码查 API），不编译 |

## VPS 上的编译流程（测试时用）

1. 在开发电脑上提交并推送（没做完的也可以推，见 [git-workflow.md](git-workflow.md)）
2. VPS 上进入对应 worktree：`git fetch origin && git checkout --detach <提交号>`
3. `source /opt/open-proxy/env.sh`（Go 1.26.5、Rust 1.96.0、Node v24.14.1 + pnpm 10，缓存都在 `cache/` 下），在 slice 里编译，要留的产物复制到 `bin/<名称>-<分支>-<短提交号>`
   - `src/panel`、`src/agent` 是两个分支的 worktree；并行工作的会话各自另建 worktree（例如 `src/agent-e2e`），不去动别人正在用的
4. 不在 VPS 上改代码：要改就回开发电脑改完再推，保证每个产物都能追溯到提交

## 占用锁

- 编译可以并行（都在 slice 里，互相最多抢内存，不会卡死系统）
- **测试**（占端口、改防火墙、跑原型和客户端）同一时间只允许一个会话：
  - 开始前查看：`ssh op-test 'cat /opt/open-proxy/LOCK 2>/dev/null || echo free'`
  - 显示 `free`：写入 `<run-id> <分支> <开发电脑名> <开始时间>` 后再开始
  - 已被占用：不要动，告诉管理员谁在用；超过 24 小时的锁，向管理员确认后才能清掉
  - 这一轮测试结束后删除 LOCK

## 端口

| 端口 | 用途 |
| --- | --- |
| 22 | SSH，不许改动 |
| 20000–29999 | 测试用入站 |
| 30000–30999 | Hysteria2 端口跳跃测试 |

其他端口不使用。需要从外网访问测试端口时，开放前先确认，这一轮结束后关闭。

## 安全规则

- 不修改 SSH 配置和账号密码；不关闭整个防火墙；不升级内核、不重装系统组件（除非管理员同意）。唯一允许的系统配置是「一次性初始化」里的 slice
- 防火墙规则只能加在专用的 nftables 表 `open_proxy_test` 里（用 iptables 时，规则带 `open-proxy-test` 注释），这一轮结束后清理
- 测试用的代理入站必须要求认证。不开无认证的 SOCKS5 / HTTP 代理，避免被扫描到当作开放代理滥用
- 不在 VPS 上扫描网段（包括 RealiTLScanner），除非专门验证扫描功能并事先得到管理员确认
- 测试凭据随机生成、用完即弃，不写进仓库
- 安装系统软件包前先确认有必要，并写进调试记录

## 进程

- 所有进程按「内存保护」一节用 `systemd-run` 在 `open-proxy.slice` 里启动；输出重定向到 run 目录
- 这一轮结束时：停止本轮启动的所有进程（`systemctl stop 'op-<run-id>-*'`），清理防火墙规则，删除 LOCK。run 目录保留
- `bin/` 和 `runs/` 里超过 30 天的内容可以删除（重要结论已经写进调试记录）

## 记录

每轮调试都要写调试记录，规则见 [debug-runs/README.md](debug-runs/README.md)。run-id 同时用作 VPS 上的 run 目录名。

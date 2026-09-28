# R20260918-main-01 初始化测试 VPS

> 摘要：按 test-vps.md 初始化 <VPS_IP>。发现机器上还跑着管理员自己的服务（服务 A、服务 B），因此把我们的内存上限从「总内存 − 1 GiB」下调到 7 GiB。slice、目录、只读 Deploy Key、仓库克隆和两个 worktree 全部就绪。

- 状态：通过
- 日期：2026-09-18；执行：Windows 开发机 / Claude Code 会话
- 关联：无 ExecPlan；开发轮次 D20260918-main-02
- 代码：main `10c9d38`
- 环境：测试 VPS，Debian 12 (bookworm) x86_64，8 核，内存 11.68 GiB，磁盘 197 GB（已用 33 GB）

## 目的

把 VPS 配置到能编译和跑原型的状态，并确认不会影响机器上已有的服务。

## 做法

1. 只读检查：cgroup 版本、内存、磁盘、已装软件、防火墙、监听端口、运行中的容器
2. 创建 `/etc/systemd/system/open-proxy.slice`，`systemctl daemon-reload && systemctl start`
3. 建目录 `/opt/open-proxy/{toolchains,cache,src,bin,tools,runs}`
4. VPS 上 `ssh-keygen -t ed25519` 生成 `/root/.ssh/open-proxy-deploy`；公钥通过 `gh api repos/yuuuki-creation/open-proxy/keys` 以只读 Deploy Key 加到仓库；`/root/.ssh/config` 配 `Host github-open-proxy`
5. `git clone` 到 `src/repo`，再为 panel、agent 建 detached worktree

## 结果

| 项 | 结果 |
| --- | --- |
| cgroup | `cgroup2fs`，支持内存控制 |
| slice 生效值 | `MemoryMax` 7 GiB、`MemoryHigh` 6.5 GiB、`MemorySwapMax` 0 |
| Go | 系统已装 `go1.26.5 linux/amd64`，和开发机同版本，直接用 |
| 仓库 | `src/repo` 在 main `10c9d38`；worktree `src/agent` = `a7cf850`、`src/panel` = `c44f0b5` |
| 现有服务 | 服务 A（前端、服务端、MongoDB、Redis，已跑 7 天，占 8091）、服务 B（Caddy 占 80 / 443，web、worker） |
| 内存现状 | 总 11.68 GiB，其他服务约 2.3 GiB，可用约 9.6 GiB |
| 端口 | 80、443、8091 被占；22 是 SSH；我们的 20000–29999、30000–30999 空闲 |

## 结论

- 初始化完成，可以开始在 VPS 上编译和验证
- **内存上限改为 7 GiB**（原计划是总内存 − 1 GiB = 10.7 GiB）。机器不是专用的，按原值我们的编译可能挤掉 MongoDB 和网站。已更新 `docs/test-vps.md`，并新增「这台机器上还有别的服务」一节
- Docker 在 `ip nat` 表里有自己的 DNAT 链。V7 端口跳跃要用独立的 `open_proxy_test` 表，届时确认两者不冲突
- 没有装 Rust 和 Node：panel 分支开工时再装

## 下一步

agent 分支建 `prototypes/agent-core` 模块，开始 V1。

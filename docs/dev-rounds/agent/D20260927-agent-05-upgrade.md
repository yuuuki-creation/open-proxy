# D20260927-agent-05 正式 Agent：M7 升级、回滚、卸载

> 摘要：`Upgrade` 下载新版本（Token 放请求头）、校验 SHA-256 和对整个文件的 Ed25519 签名、试跑 `-version`、备份、写升级标记、原子替换，回复并补发流量上报后退出；新版本 3 分钟内没通过认证就换回旧版本，旧版本在 Hello 里报 `rolled_back_from`。`Uninstall` 和「服务器已删除」时停入站、删 nftables 表和文件、让 systemd 停掉服务。测试 VPS 上两轮运行验证通过。经 PR #15 合并。

- 状态：完成
- 关联：[ExecPlan](../../exec-plans/completed/2026-09-23-agent.md) M7；调试记录：[R20260927-agent-04](../../debug-runs/agent/R20260927-agent-04-upgrade.md)、[R20260927-agent-05](../../debug-runs/agent/R20260927-agent-05-upgrade-retest.md)
- 提交：工作分支 `agent-upgrade`（`953c8b9`..`551991e`）squash 合并为 `28f5e52`（PR #15）

## 目的

按计划 M7 和 protocol.md「升级」「删除服务器」：主控能远程升级 Agent，新版本起不来时自动回滚；删除服务器时 Agent 卸载自己。

## 做了什么

- `agent/internal/upgrade/upgrade.go`：下载（只用自己配置的主控地址、不跟随跳转、上限 256 MiB）、SHA-256、Ed25519 验签（公钥编译时注入 `main.upgradePublicKey`，为空拒绝）、检查 ELF 架构并试跑 `-version`、硬链接备份 `.bak`、写升级标记、原子替换；启动时看标记（试运行、启动太多次直接回滚、上次升级没生效就报 `rolled_back_from`）；回滚
- `agent/internal/upgrade/uninstall.go`：删二进制、备份、配置、数据里自己的文件；从 `/proc/self/cgroup` 找到自己的服务，disable、删服务文件、`systemctl stop --no-block`
- `agent/internal/conn/`：`Handler` 加 `AfterReply`，回复发出后再退出
- `agent/cmd/op-agent/`：`main.go` 先看升级标记再读配置，试运行的计时，按退出原因（升级、回滚、卸载、服务器已删除）收尾；`handler.go` 接上 `Upgrade`、`Uninstall`、Hello 的 `rolled_back_from`
- `agent/test/`：假主控加 `-keygen`、`-sign-key`、`-reject-version`、`/ctl/upgrade`；脚本 `scripts/m7-upgrade.sh`
- 取舍写在计划的「决策记录」（2026-09-27）

## 结果

- 测试 VPS 上 gofmt、go vet、staticcheck、两种架构编译通过；PR #15 的 CI 通过，已合并
- 运行验证见两条调试记录；第一轮发现 `systemctl stop --no-block` 被停止任务一起杀掉、被误报成失败，改后复测通过

## 遗留问题

- 新版本如果在读升级标记之前就崩溃（例如启动时 panic），它没法自己回滚，会被 systemd 反复拉起；要靠 systemd 的启动次数限制或安装脚本兜底，记为技术债 agent-6
- 发布流程定下来前，正式编译不注入公钥，等于不允许升级（计划「接口与依赖」）

## 下一轮

M8：REALITY 目标检测与扫描。

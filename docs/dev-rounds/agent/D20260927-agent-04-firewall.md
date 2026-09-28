# D20260927-agent-04 正式 Agent：M6 端口跳跃

> 摘要：Hysteria2 端口跳跃用 nftables 实现：一张 inet 表、一条 nat 链，每个节点一条规则把发往本机的 IPv4 UDP 端口范围转到节点实际的端口；每次应用整体重建（一次原子提交），冲突的节点单独报 `ITEM_PORT_HOPPING`。测试 VPS 上用 `nft` 读回验证通过。经 PR #12 合并。

- 状态：完成
- 关联：[ExecPlan](../../exec-plans/completed/2026-09-23-agent.md) M6；调试记录：[R20260927-agent-03](../../debug-runs/agent/R20260927-agent-03-port-hopping.md)
- 提交：工作分支 `agent-firewall`（`c4f2599`..`ec91a20`）squash 合并为 `ccad354`（PR #12）

## 目的

按计划 M6 和 nodes.md「Hysteria2 端口跳跃与混淆」：把节点的端口跳跃范围转到 Hysteria2 的实际端口；启动时和每次应用时幂等重建，删节点时清理，失败按逐项应用上报。

## 做了什么

- `agent/internal/firewall/firewall.go`：`Apply` 先加再删表、再建表链规则，一次提交；`Remove` 删表（M7 卸载用）
- `agent/internal/state/apply.go`：第 7 步 `applyPortHopping`，目标用节点实际在跑的端口；范围不合法、重叠、盖住别的节点的端口、节点没在跑时只有它失败；提交失败时开了端口跳跃的节点都失败
- `agent/cmd/op-agent/main.go`：`-nft-table` 参数；没有本地状态时启动就清表
- `agent/test/`：假主控自动填自签证书；`scripts/m6-firewall.sh`
- 取舍写在计划的「决策记录」（2026-09-27）

## 结果

- 测试 VPS 上 gofmt、go vet、staticcheck、两种架构编译通过；PR #12 的 CI 通过，已合并
- 运行验证见调试记录：规则内容、整体重建、冲突检查、重启和没有本地状态时的清理都符合预期；测试 VPS 上装了 nftables 包（只用来读规则，服务未启用）

## 遗留问题

- 真实的 UDP 转发要从外网发包才能测，留到集成测试（技术债 agent-2）
- nodes.md 写的「没有 nftables 时用 iptables」没做，见决策记录；main 的 architecture.md「未验证的假设」里 nftables 那一项已验证，待 main 更新

## 下一轮

M7：升级、回滚、卸载。

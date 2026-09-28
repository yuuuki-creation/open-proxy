# D20260927-agent-01 技术债 agent-3：重建失败时保持原样

> 摘要：端口不变的重建改为「先校验、再删旧建新、失败时按原来的配置建回去」，证书改为单独校验的一项，补上 protocol.md 要求的应用前校验。经 PR #7 合并，CI 全部通过。另外发现 Agent 重启后失败项不再保持原样，记为技术债 agent-4。

- 状态：完成
- 关联：[ExecPlan](../../exec-plans/completed/2026-09-23-agent.md) 决策记录（2026-09-27）；技术债 agent-3、agent-4；调试记录：无
- 提交：`04dfd51`、`036c19d`（文档）；`7c5079b`（工作分支 `agent-rebuild`）squash 合并为 `558ff0a`（PR #7）

## 目的

做到 protocol.md「应用规则」的「失败项保持原样」和「应用前校验」：端口不变的节点和自建落地重建失败时不再停掉；证书有问题时只报证书这一项，TLS 节点接着用原来的证书。用户选了 agent-3 的办法 2（失败时按原来的配置建回去），再补上应用前校验。

## 做了什么

- `agent/internal/core/core.go`：新增 `RebuildInbound`，删旧入站之前先用 sing-box 的入站注册表构造一遍新入站（不启动、不占端口）来校验；删旧建新失败时按原来的配置建回去，两次都失败才返回 `ErrInboundStopped`。`SetInbound` 去掉 `removeFirst`，只用于新建和端口变了的入站
- `agent/internal/core/options.go`：`CheckCertificate` 用 `tls.X509KeyPair` 校验证书，和 sing-box 构造 TLS 入站时一样
- `agent/internal/state/`：记下每个节点和自建落地正在运行的入站配置；应用顺序里在节点之前加了「证书」一步，校验不过报 `ITEM_CERTIFICATE`
- 对照 sing-box v1.14.1 源码确认：这 5 种入站只构造、不启动时不监听端口，也不起 goroutine，没启动就 `Close` 是安全的

## 结果

- PR #7 的检查全部通过（gofmt、go vet、staticcheck、linux amd64 / arm64 编译），已合并；技术债 agent-3 已解决
- 没有运行测试：端口不变的重建已写进计划的「验证与验收」，等集成测试时实测

## 遗留问题

- 技术债 agent-4：本地只保存最新的期望状态，Agent 重启后，之前校验不过的项按新配置建，照样失败，原来在运行的也起不来。要改本地保存的内容，会动到 main 的 architecture.md「Agent 的文件」，这一轮没做

## 下一轮

M4：流量上报（计数快照、网卡、`boot_id`，每 10 秒发 `TrafficReport`）。

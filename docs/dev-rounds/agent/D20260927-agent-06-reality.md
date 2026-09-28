# D20260927-agent-06 正式 Agent：M8 REALITY 目标检测与扫描

> 摘要：`CheckRealityTargets` 对每个目标握手 3 次，报 TLS 1.3、H2、证书是否有效和延迟；`ScanRealityTargets` 按并发数和速率连网段里各地址的 443 端口，结束时一次性返回支持 TLS 1.3 和 H2 的候选。测试 VPS 上两轮运行验证通过（扫描只扫本机回环网段）。经 PR #16 合并。至此计划的 8 个阶段全部完成，计划移到 completed。

- 状态：完成
- 关联：[ExecPlan](../../exec-plans/completed/2026-09-23-agent.md) M8；调试记录：[R20260927-agent-06](../../debug-runs/agent/R20260927-agent-06-reality.md)、[R20260927-agent-07](../../debug-runs/agent/R20260927-agent-07-reality-retest.md)
- 提交：工作分支 `agent-reality`（`8e74bad`..`6d56c94`；M7 合并前先在临时分支 `agent-reality-wip` 上测过，再原样挑过来）squash 合并为 `6f92d87`（PR #16）

## 目的

按计划 M8 和 nodes.md「REALITY 伪装目标」：管理员选中的伪装目标由 Agent 在节点服务器上检测；需要时让 Agent 慢速扫描所在网段找候选。

## 做了什么

- `agent/internal/reality/reality.go`：`Check`（最多 32 个目标、8 个并行、50 秒内完成；每个目标先解析一次、再 3 次握手，证书自己验证不中断握手，结果取最保守的，延迟取中位数）、`Scan`（只扫 IPv4、最多 4096 个地址、并发默认 4 上限 32、速率默认每秒 4 个上限 50，最坏情况超过 18 分钟就拒绝；不发 SNI，合格条件 TLS 1.3 + H2 + 证书里有不带通配符的域名；按延迟排序）
- `agent/cmd/op-agent/handler.go`：接上两个请求；扫描同一时间只做一个；请求不对或扫描失败回 `ErrorReply`
- `agent/test/`：假主控加 `-tls-listen` 测试 TLS 服务；脚本 `scripts/m8-reality.sh`
- 取舍写在计划的「决策记录」（2026-09-27）

## 结果

- 测试 VPS 上 gofmt、go vet、staticcheck、两种架构编译通过；PR #16 的 CI 通过，已合并
- 运行验证见两条调试记录；第一轮发现延迟里含了每次的 DNS 查询，改为先解析一次后复测通过
- 计划填了「结果与复盘」，移到 `docs/exec-plans/completed/`，本分支文档里指向它的链接一起改了

## 遗留问题

- 真实网段的扫描没在测试 VPS 上做（test-vps.md 要求事先得到管理员确认），留给集成测试
- `internal/reality/` 和 `agent/test/` 不在 code-style.md 的目录表里，需要 main 补

## 下一轮

计划完成。下一步是另写计划，在测试 VPS 上做集成测试（技术债 agent-2），以及处理技术债 agent-4、agent-5、agent-6。

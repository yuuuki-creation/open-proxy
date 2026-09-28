# D20260923-agent-01 正式 Agent：写计划，完成 M1 骨架

> 摘要：写了正式 Agent 的执行计划（8 个阶段）。写计划时发现 sing-box 的路由规则不能在运行时修改，落地出口改用「分发」出站，已同步到 main 的设计文档。M1 骨架经 PR #3 合并，CI 第一次就全部通过。

- 状态：完成
- 关联：[ExecPlan](../../exec-plans/completed/2026-09-23-agent.md) M1；调试记录：无
- 提交：`5d7147c`（计划）、`7abfbb1`（M1，PR #3）

## 目的

开始写正式 Agent：先定计划，再搭好骨架和 CI，后面每个阶段只管写功能。

## 做了什么

- 计划 `docs/exec-plans/completed/2026-09-23-agent.md`：组成、数据流、逐项应用的顺序、8 个阶段各自的工作分支和文件
- main 的 `architecture.md`：落地出口改用分发出站、Mieru 连接交给 sing-box 路由、Agent 的文件和配置格式、「未验证的假设」加两条（提交 `3eccab9`）
- `agent/cmd/op-agent/`：程序入口、读配置、slog 日志、日志里隐去 Token
- `agent/buf.gen.yaml`、`agent/internal/pb/`：protobuf 代码生成
- `.github/workflows/agent.yml`：指向 agent 的 PR 上检查生成代码、格式、vet、staticcheck、两种架构的编译

## 结果

- PR #3 的检查全部通过，已合并
- 设计上的变化和原因记在计划的「意外发现」「决策记录」里

## 遗留问题

- 分发出站、Mieru 交给 sing-box 路由都只核对了源码里的接口，运行时没验证，列在 architecture.md「未验证的假设」

## 下一轮

M2：连主控（WebSocket、Hello、重连退避、请求和回复）。

# D20260923-main-02 设计并定义通信协议

> 摘要：写出主控与 Agent 的通信协议设计，管理员定了 4 项（主控生成自签证书、逐项应用、网速 10 秒刷新、删除服务器时 Agent 自动卸载）；`protocol/` 下三个 .proto 经 PR #1 合并进 main，PR 上的 buf 检查已验证能拦住不兼容改动。

- 状态：完成
- 关联：[ExecPlan](../../exec-plans/completed/2026-09-23-protocol.md)（已完成）；调试记录：无
- 提交：`3fd145f`..本记录所在的提交（含 PR #1 合并的 `10321e4`）

## 目的

定下主控和 Agent 之间的全部消息，让 panel、agent 两边的正式代码可以开工。

## 做了什么

- `docs/design-docs/protocol.md`：外壳、消息清单、连接流程、期望状态结构、应用规则、上报、升级、删除服务器、兼容规则
- 4 项决定同步进 `architecture.md`（应用配置、Token、待讨论）、`nodes.md`、`subscription.md`（自签证书）、`panel.md`（网速、删除）
- `protocol/openproxy/agent/v1/`：`envelope.proto`、`stable.proto`、`agent.proto`；`protocol/buf.yaml`
- `.github/workflows/protocol.yml`：指向 main 的 PR 上跑 `buf lint`、`buf breaking`；`docs/git-workflow.md` 补上「改 protocol/ 走 PR」
- 第一次走「工作分支 + PR」流程：PR #1 合并；PR #2 故意改坏兼容性，确认检查失败后关闭

## 结果

- 协议定义进了 main，panel、agent 合并 main 后就能拿到
- 设计过程中发现并补上两个缺口，记在 ExecPlan 的「意外发现」

## 遗留问题

- Go 和 Rust 的代码生成还没接，放到 agent、panel 各自的计划里
- 主控 API、数据库表结构、Agent 配置文件格式、发布打包仍在 `architecture.md`「待讨论」
- 代码规范还没定

## 下一轮

讨论主控 API 结构和数据库表结构（panel 开工的前提）；agent 可以开始写正式 Agent 的 ExecPlan。

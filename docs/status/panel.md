# panel 分支状态

> 新会话先读 `AGENTS.md` 和本文件，通常就够开始工作。本文件不超过 40 行，每轮开发结束时**整篇重写**，不要在末尾追加。

- 更新：2026-09-23，合并 main：代码规范已定（记录在 main：[D20260923-main-04](../dev-rounds/main/D20260923-main-04-code-style.md)）
- 本分支内容：主控后端 `master/`（Rust）、前端 `web/`（React + HeroUI）
- 目前还没有代码，也还没有开发轮次；设计在 `docs/design-docs/`：architecture（技术栈、通信、流量与超额）、panel（页面）、subscription（订阅）、nodes（节点与证书）

## 当前阶段

- **先把正式代码写完，再统一测试**（2026-09-23 决定）：测试阶段之前用不到测试 VPS
- 编译放在 GitHub Actions 上：改代码走工作分支 + PR，PR 上自动编译（见 `docs/git-workflow.md`「PR 与编译」）

## 下一步

1. 主控开工需要的设计都已定：通信协议（`protocol.md`、`protocol/openproxy/agent/v1/`）、数据库表结构（`database.md`）、主控 API（`api.md`）
2. 还没定的订阅设计：`subscription.md`「待讨论」里的模板翻译子集
3. 待调研：Rust 的 YAML 库（serde_yaml 已停止维护）、ACME 库
4. 按 `docs/PLANS.md` 写主控的 ExecPlan（规范见 `docs/code-style.md`），再动代码；第一个代码 PR 里建 `.github/workflows/panel.yml`，并接入 `protocol/` 的 Rust 代码生成（prost）

## 阻塞

- 无

## 本分支要点

- 测试阶段在 VPS 上编译时，进程必须跑在 `open-proxy.slice` 里（给系统留 1 GiB）；Rust 编译很吃内存，注意 `CARGO_BUILD_JOBS`；VPS 上还没装 Rust、Node
- 不在 VPS 上改代码：改完在开发电脑提交推送，VPS 拉取指定提交再编译
- 记录：开发轮次 `docs/dev-rounds/panel/`，调试 `docs/debug-runs/panel/`，编号带 `panel`
- 设计文档、协议定义属于 main：需要改就到 main 上改，再 `git merge origin/main`
- 主控不得引用 sing-box、Mieru 或 agent 分支的代码，避免受 GPL 约束

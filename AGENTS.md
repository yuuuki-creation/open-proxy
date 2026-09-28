# AGENTS.md

给 AI 编码代理（Codex、Claude Code、Cursor 等）读的项目入口。本文件只当**目录**用：保持在 100 行以内，细节写进 `docs/`，这里只放指向。

## 项目概览

- **open-proxy**：轻量、简化版的[妙妙屋 X](https://miaomiaowux.com/docs/)，集中管理多台代理服务器、节点、用户和订阅
- 组成：主控（Rust 后端 + React / HeroUI 前端，SQLite）；Agent（Go，内嵌 sing-box 和 Mieru）
- 运行环境：Linux 服务器（amd64 / arm64）
- 范围和核心选型见 `docs/design-docs/product-scope.md`

## 分支（动手前先确认自己在哪个分支）

| 分支 | 内容 |
| --- | --- |
| `main` | 只放文档、规范和通信协议定义 `protocol/`，没有代码 |
| `panel` | 主控后端 `master/`（Rust）、前端 `web/`（React） |
| `agent` | Agent `agent/`（Go）、原型 `prototypes/` |

每个文件只属于一个分支，只在所属分支上改；只允许 main 合并进 panel / agent，代码不进 main。改代码走工作分支 + PR，编译和测试在测试 VPS 上做，PR 上的 CI 是合并前的最后检查。提交信息用中文，轮中也要及时推送。详见 `docs/git-workflow.md`。

## 常用命令

见 `docs/code-style.md` 的「常用命令」（代码初始化后补全）。编译和测试都在测试 VPS 上做（见 `docs/test-vps.md`）；改代码开 PR 后 GitHub Actions 自动再检查一遍（见 `docs/git-workflow.md`）。

## 仓库结构

| 路径 | 用途 |
| --- | --- |
| `AGENTS.md` / `CLAUDE.md` | 项目入口与索引 |
| `docs/git-workflow.md` | 分支、文件归属、PR 与编译、合并、推送、提交信息 |
| `docs/code-style.md` | 代码规范：语言、程序名、各部分的工具和目录、CI 检查、常用命令 |
| `docs/test-vps.md` | 测试 VPS 使用规范（内存保护、占用锁、安全规则） |
| `docs/PLANS.md` | ExecPlan（执行计划）的写法规范 |
| `docs/design-docs/` | 架构与设计决策，入口 `index.md` |
| `docs/status/<分支>.md` | 各分支的当前状态：阶段、下一步、阻塞，40 行以内 |
| `docs/dev-rounds/` | 开发轮次记录：规则见 `README.md`，记录在 `<分支>/` 子目录 |
| `docs/debug-runs/` | 调试记录：规则见 `README.md`，记录在 `<分支>/` 子目录 |
| `docs/exec-plans/active/`、`completed/` | 进行中 / 已完成的计划，放在所属分支上 |
| `docs/exec-plans/tech-debt/` | 技术债清单：规则见 `README.md`，每个分支一个文件 |
| `protocol/` | 主控与 Agent 的通信协议定义（Protocol Buffers，main 维护）；设计见 `docs/design-docs/protocol.md` |
| `prototypes/` | 原型验证代码，不是正式实现（agent 分支） |

## 测试服务器

| 项 | 值 |
| --- | --- |
| 位置 / 配置 | 荷兰，8 核 / 12 GB 内存 / 200 GB 硬盘 |
| IP | <VPS_IP>（各开发电脑自己配，不进仓库） |
| SSH 用户 | root |
| 系统 | Debian 13 (trixie)，x86_64（2026-09-27 重装） |

编译、验证和测试都在这台机器上做。使用前必读 `docs/test-vps.md`：内存保护（所有进程跑在 `open-proxy.slice` 里，上限 7 GiB）、SSH 别名 `op-test`、编译流程、占用锁、端口、安全规则。SSH 密钥由各开发电脑自己配置。

## 读文档的顺序（省 token）

1. 新会话先读本文件和当前分支的 `docs/status/<分支>.md`，够用就停。看别的分支状态不用切换：`git show origin/agent:docs/status/agent.md`。
2. 需要历史时，先读对应目录里本分支的 `index.md`，只打开相关的记录，而且先读前 15 行（标题、摘要、状态）。
3. 长文档（设计文档、参考笔记）先读开头的摘要，再按 `## ` 小节标题搜索，只读需要的那一节，不整篇读。
4. 不要把其他文档用 `@` 引用进 `CLAUDE.md`（`AGENTS.md` 本身除外）：引用的内容每次启动都会全部加载。

## 工作流程

1. **开工前**：`git fetch`、`git pull --rebase`，合并 `origin/main`；再看 `docs/status/<分支>.md` 和 `docs/exec-plans/active/`，有进行中的计划就接着做。
2. **复杂任务**（新功能、较大重构、预计跨多个会话）先按 `docs/PLANS.md` 写 ExecPlan，放进 `docs/exec-plans/active/`，再动代码。
3. **实施中**持续更新计划的「进度」「意外发现」「决策记录」，让计划始终反映真实状态：任何人或代理只读这份文件就能接手。
4. **完成后**填写「结果与复盘」，把文件移到 `docs/exec-plans/completed/`。
5. 发现但不在本次范围内的问题，记进 `docs/exec-plans/tech-debt/<分支>.md`，不要扩大本次改动范围。
6. 做出影响整体结构的决定时，在 main 上写设计文档并更新 `index.md`，再把 main 合并回当前分支。
7. 研究参考项目学到的东西记进 `reference/<项目>/`，写明来源。参考源码只学思路，动手前先读该项目 README 里的许可证约束（妙妙屋 X 的代码一律不能复制）。
8. **编译和调试**：编译、检查和测试都在测试 VPS 上做，先读 `docs/test-vps.md`；开发电脑不编译（解析依赖可以）。开 PR 后 GitHub Actions 自动再检查一遍，是合并前的最后检查（见 `docs/git-workflow.md`）。每一轮调试按 `docs/debug-runs/README.md` 写记录，先写目的和做法，跑完立即补结果和结论。
9. **每轮开发结束**：按 `docs/dev-rounds/README.md` 写本轮记录并更新本分支索引，整篇重写 `docs/status/<分支>.md`，然后提交推送。
10. **写文档省 token**：摘要放在标题下第一段；每类信息只写一处（见下一节）；不贴代码、diff 和大段日志；遵守各类记录的篇幅上限。

## 每类信息只写一处

同一个结论写在多处，改的时候总会漏掉几处。每类信息只放在一个地方，别处只写一句话加链接：

| 内容 | 只写在 |
| --- | --- |
| 测量数据、调试过程 | 调试记录 `docs/debug-runs/` |
| 设计结论、未验证的假设 | 设计文档 `docs/design-docs/`（main） |
| 计划的进度、意外发现 | ExecPlan；影响设计的发现，结论写进设计文档 |
| 当前阶段、下一步、阻塞 | `docs/status/<分支>.md`，不列结论 |
| 这一轮改了什么 | 开发轮次记录，不复述结论 |

## 规则

- 不要提交密钥、token、`.env` 等敏感信息。
- 行为变了就同步更新相关文档；发现文档与代码不一致时，以代码为准修正文档。
- 写代码前读 `docs/code-style.md`。要点：标识符英文，注释、日志、错误信息中文；程序名 `op-master`、`op-agent`；主控和前端不得依赖 GPL 系的库；写代码阶段先不写单元测试。

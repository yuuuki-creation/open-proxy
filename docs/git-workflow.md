# Git 工作流

> 摘要：三个长期分支。main 只放文档和规范（含协议定义）；panel 放主控后端和前端；agent 放 Agent 和原型。每个文件只在所属分支上改，只允许 main 合并进 panel / agent，代码永不进 main。改代码走工作分支 + PR，编译在 GitHub Actions 上跑。提交信息用中文；每轮结束必须推送，轮中也要及时推送。

## 分支

| 分支 | 内容 |
| --- | --- |
| `main` | 设计文档、规范、参考笔记、`AGENTS.md` / `CLAUDE.md`、通信协议定义 `protocol/` |
| `panel` | 主控后端 `master/`（Rust）、前端 `web/`（React） |
| `agent` | Agent `agent/`（Go）、原型 `prototypes/` |

## 文件归属

每个文件只属于一个分支，只在所属分支上修改。这样 main 合并进 panel / agent 时不会冲突。

| 归属 | 文件 |
| --- | --- |
| `main` | `AGENTS.md`、`CLAUDE.md`、根目录 `.gitignore`、`protocol/`、`.github/workflows/protocol.yml`、`reference/`、`docs/design-docs/`、`docs/PLANS.md`、`docs/git-workflow.md`、`docs/test-vps.md`、`docs/code-style.md`、各记录目录的 `README.md`、`docs/status/main.md`、`docs/dev-rounds/main/`、`docs/debug-runs/main/`、在 main 上创建的 ExecPlan |
| `panel` / `agent`（下面记作 `<分支>`） | 本分支的代码目录（包括目录里自己的 `.gitignore`）；`.github/workflows/<分支>.yml`；`docs/status/<分支>.md`；`docs/dev-rounds/<分支>/`；`docs/debug-runs/<分支>/`；`docs/exec-plans/tech-debt/<分支>.md`；在本分支创建的 ExecPlan（`docs/exec-plans/active/`、`completed/` 下的对应文件） |

- 在 panel / agent 上工作时，如果需要改属于 main 的文件（例如原型结论要写回设计文档、要改协议定义）：到 main 上改完提交推送，再把 main 合并回当前分支
- 不想来回切换分支，可以用 worktree 同时打开 main：`git worktree add ../open-proxy-main main`
- 协议定义放在 main，是因为主控和 Agent 必须用同一份；生成的代码各自放在 panel、agent 的代码目录里

## PR 与编译

编译放在 GitHub Actions 上，开发电脑和测试 VPS 都不负责编译检查。

- 改代码：从 panel 或 agent 拉一个工作分支，命名 `<分支>-<简短英文标识>`（例如 `agent-tracker`；不能用 `agent/tracker`，会和 `agent` 分支名冲突）。推送后开 PR，合并回它出发的分支
- PR 上自动编译和静态检查，各分支查什么见 [code-style.md](code-style.md)「CI 检查」。通过再合并，合并后删掉工作分支
- 工作流文件是 `.github/workflows/<分支>.yml`，属于对应分支，在该分支的第一个代码 PR 里建
- 改 `protocol/`：从 main 拉工作分支 `main-<简短英文标识>`，开 PR 到 main。PR 上跑 `buf lint` 和 `buf breaking`（`.github/workflows/protocol.yml`），通过再合并
- 只改文档（记录、状态、计划）不用开 PR，直接推送到对应分支，main 也一样

## 合并

- 只有一个方向：`main → panel`、`main → agent`。工作分支通过 PR 合并回 panel / agent 不算在内
- 不把 panel / agent 合并进 main；panel 和 agent 之间也不合并
- 时机：每轮开发开始时；以及 main 上改了当前分支需要的文件之后。命令：`git fetch && git merge origin/main`
- 合并提交的信息：`合并 main：<带来了什么>`

## 推送

- 每轮开发结束必须推送
- 轮中也要及时推送，至少在这些时候：
  - 完成一个可以单独说明的步骤
  - 写完一条调试记录
  - 要在测试 VPS 上编译或测试之前（VPS 从 GitHub 拉代码）
  - 结束会话、换电脑之前
- 工作分支上允许推送没做完的代码，提交标题里标「【进行中】」
- 不强制推送，不改写已经推送的历史
- 开始工作前先同步：`git fetch`，然后 `git pull --rebase`

## 提交信息

- 用中文。格式：

```
<范围>: <做了什么>

<正文（可选）：为什么这样改；关联的开发轮次、计划或调试记录>
```

- 范围：`docs`、`protocol`、`master`、`web`、`agent`、`prototype`、`build`
- 标题一句话，动词开头，不超过 50 个字；没做完的在冒号后加「【进行中】」
- 例子：
  - `docs: 增加测试 VPS 使用规范`
  - `prototype: 【进行中】连接追踪层按用户计数`
  - `prototype: 完成 V1，四种入站在 VPS 上连通`
  - `合并 main：更新测试 VPS 规范`
- AI 提交时，按所用工具的要求在末尾加署名行
- 2026-09-17 之前的几次提交是英文，不改写历史

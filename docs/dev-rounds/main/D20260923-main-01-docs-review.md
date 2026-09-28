# D20260923-main-01 整理文档规范，原型阶段收尾

> 摘要：检查进度和文档后，改了几条规范：每类信息只写一处、编译放到 GitHub Actions（改代码走工作分支 + PR）、main 上也能写 ExecPlan、测试脚本必须进仓库。决定原型到 V4 为止，先写完正式代码再统一测试，没实测的假设集中列进架构文档。顺带清掉过时和前后矛盾的内容。

- 状态：完成
- 关联：原型计划（agent 分支）收尾；调试记录：无
- 提交：本记录所在的提交；随后 agent、panel 两个分支各有一个收尾提交

## 目的

检查整体进度，看 `AGENTS.md` 和 `docs/` 的规范是否合理，改掉发现的问题。

## 做了什么

- `AGENTS.md`：新增「每类信息只写一处」；编译改在 GitHub Actions 上做；说明 `CLAUDE.md` 引用 `AGENTS.md` 是例外；代码规范的 TODO 写明开工前要定哪些内容
- `docs/git-workflow.md`：新增「PR 与编译」：改代码从 panel / agent 拉工作分支、开 PR，PR 上自动编译；工作流文件归各自分支
- `docs/test-vps.md`：新增「编译和测试的分工」，VPS 只做验证和测试；run 目录只放运行时的输出
- `docs/debug-runs/README.md`：测试脚本必须提交进仓库
- `docs/dev-rounds/README.md`：只同步结论的小改动不算一轮；轮次记录不复述结论
- `docs/PLANS.md`、`docs/git-workflow.md`：main 上也可以写 ExecPlan；模板加「分支」一行
- `docs/design-docs/architecture.md`：新增「未验证的假设」；「停用用户」和「用户变更」两节的说法统一；「待讨论」去掉过时和重复的条目，补上通信协议、Agent 本地文件、发布打包
- `docs/design-docs/product-scope.md`：删掉和 `subscription.md` 重复的格式表、过时的「下一步」；设计文档索引的日期改为「最后更新」

## 结果

- 同一个结论以前会写在多达 9 个地方（例如「Hysteria2 重建入站断会话」），以后按新规则只写一处
- 原型计划在 agent 分支收尾，移到 `completed/`；panel 分支合并 main，状态重写

## 遗留问题

- 已有记录里的重复内容不回头改，只在以后的记录里遵守新规则
- 代码规范还没定
- Windows 开发电脑的 SSH 别名是 `testvps`，还没改成 `op-test`

## 下一轮

设计通信协议：列出主控和 Agent 之间的消息，建 `protocol/`。

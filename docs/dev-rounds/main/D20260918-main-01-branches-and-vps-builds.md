# D20260918-main-01 分支模型、提交规范、编译上 VPS

> 摘要：确定三分支模型（main 只放文档规范，panel 放主控和前端，agent 放 Agent 和原型）和文件归属规则，提交信息改用中文、要求及时推送；编译和测试统一放到测试 VPS，并强制给系统预留 1 GiB 内存。文档记录改成按分支分目录。

- 状态：完成
- 关联：无；调试记录：无
- 提交：本记录所在的提交，以及随后 agent、panel 两个分支的初始提交

## 目的

为多台电脑、两条并行开发线（主控和 Agent）定下分支、提交、编译和记录的规矩，避免文档互相冲突、VPS 被编译撑爆。

## 做了什么

- 新增 `docs/git-workflow.md`：三分支、文件归属表、只允许 main 合并进两个分支、推送时机、中文提交信息格式
- 记录按分支分目录：`docs/status/<分支>.md`、`docs/dev-rounds/<分支>/`、`docs/debug-runs/<分支>/`、`docs/exec-plans/tech-debt/<分支>.md`；各目录的 `README.md` 存规则，编号里加分支（如 `D20260918-agent-01`）
- 重写 `docs/test-vps.md`：编译也在 VPS 上做；新增「内存保护」和「一次性初始化」两节（`open-proxy.slice` 上限为总内存 − 1 GiB，所有进程必须跑在里面）；新增编译流程、工具链和缓存目录；测试才需要占用锁
- `docs/design-docs/architecture.md` 补记之前口头定下的技术栈默认做法；`protocol/` 归 main 维护
- `AGENTS.md` 加分支一节，更新结构表、阅读顺序和工作流程
- 原型 ExecPlan 从 main 移到 agent 分支

## 结果

- main 上的文档结构和规范就绪；panel、agent 两个分支已创建并推送初始内容

## 遗留问题

- 测试 VPS 的连接信息待填、VPS 待初始化
- 主控 API 结构、模板翻译子集、数据库表结构仍未讨论

## 下一轮

VPS 就绪后，在 agent 分支开始原型 V1。

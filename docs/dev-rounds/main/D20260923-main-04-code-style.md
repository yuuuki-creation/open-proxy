# D20260923-main-04 定代码规范

> 摘要：新增 `docs/code-style.md`。管理员定了 3 项：注释、日志、错误信息全部用中文；写代码阶段先不写单元测试；程序名 `op-master` 和 `op-agent`。其余按常规做法定：各部分的格式化和检查工具、目录结构、生成代码提交进仓库、用 cargo-deny 守住主控的 GPL 边界。

- 状态：完成
- 关联：无 ExecPlan；调试记录：无
- 提交：本记录所在的提交

## 目的

panel、agent 写正式代码前，定下统一的规范。

## 做了什么

- `docs/code-style.md`：通用规则（语言、程序名、日志里隐去秘密、生成代码、依赖许可证、测试）；主控、前端、Agent 各自的工具和目录；各分支 CI 查什么；常用命令
- `AGENTS.md`：「常用命令」和「规则」里的 TODO 换成指向 code-style.md；仓库结构表加一行
- `docs/git-workflow.md`：CI 细节改为链接到 code-style.md，不再写两遍；文件归属表加上 code-style.md

## 结果

- panel、agent 开工需要的设计和规范都齐了

## 遗留问题

- 常用命令是计划中的写法，代码初始化后按实际补全
- 还没定的设计：订阅模板翻译子集、Agent 配置文件格式、发布打包；都不挡开工

## 下一轮

panel、agent 各自写正式代码的 ExecPlan。

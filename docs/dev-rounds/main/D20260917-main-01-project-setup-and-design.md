# D20260917-main-01 项目初始化、调研与设计讨论

> 摘要：从零确定 open-proxy 做什么、怎么做。建好仓库和文档结构，研究妙妙屋 X 的文档和源码，和管理员逐项讨论并写完第一版的设计文档、测试 VPS 规范、记录规范和原型计划。没有写代码。

- 状态：完成
- 关联：无（本轮产出了第一份 ExecPlan）；调试记录：无
- 提交：`4dc7758` 起，到本记录所在的提交

## 目的

确定产品范围、技术选型和架构，为写代码做好准备。

## 做了什么

- 仓库：GitHub `yuuuki-creation/open-proxy`（私有）；`AGENTS.md` + `CLAUDE.md` + `docs/` 结构
- 调研：
  - 妙妙屋 X 官方文档和四个源码仓库 → `reference/mmwx/`（文档笔记 5 篇、源码笔记 4 篇）
  - sing-box、Mieru 的接口和许可证；Rust 代理实现（shoes）→ 结论写进 architecture.md
- 设计文档（`docs/design-docs/`）：product-scope、architecture、subscription、nodes、panel
- 规范：`docs/test-vps.md`、`docs/debug-runs/`、`docs/dev-rounds/`、`docs/status/`
- 计划：原型验证 ExecPlan（2026-09-18 起移到 agent 分支）

## 结果

- 核心选型：主控 Rust + React / HeroUI；Agent Go，内嵌 sing-box 1.14.1 和 Mieru
- 第一版范围和各模块设计都有了文档，已经可以开始原型验证

## 遗留问题

- 测试 VPS 的连接信息待管理员填写
- 未讨论：模板翻译支持的子集、数据库表结构、主控 API 结构
- 待调研：Rust 的 YAML 库（serde_yaml 已停止维护）、ACME 库；sing-box REALITY 服务端需要的构建标签

## 下一轮

D20260918-main-01：定分支模型和提交规范，编译改到测试 VPS。

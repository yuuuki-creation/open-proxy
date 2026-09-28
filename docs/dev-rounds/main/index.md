# main 分支的开发轮次

| round-id | 日期 | 标题 | 状态 | 摘要 |
| --- | --- | --- | --- | --- |
| [D20260927-main-01](D20260927-main-01-vps-reinit.md) | 2026-09-27 | VPS 重装后重做初始化 | 完成 | VPS 重装成 Debian 13 后重做一次性初始化，已能编译和测试；AGENTS.md、test-vps.md 的机器信息已更新 |
| [D20260923-main-04](D20260923-main-04-code-style.md) | 2026-09-23 | 定代码规范 | 完成 | 新增 code-style.md；注释、日志、错误信息全部中文；先不写单元测试；程序名 op-master、op-agent |
| [D20260923-main-03](D20260923-main-03-api-and-schema.md) | 2026-09-23 | 设计数据库表结构和主控 API | 完成 | database.md、api.md 定稿；删除用户时流量一起删、到期精确到日、首次初始化不加保护码；不存「停用已生效」 |
| [D20260923-main-02](D20260923-main-02-protocol.md) | 2026-09-23 | 设计并定义通信协议 | 完成 | protocol.md 定稿，4 项决定；三个 .proto 经 PR #1 合并；PR 上的 buf 检查验证可用 |
| [D20260923-main-01](D20260923-main-01-docs-review.md) | 2026-09-23 | 整理文档规范，原型阶段收尾 | 完成 | 每类信息只写一处；编译放到 GitHub Actions，改代码走 PR；原型到 V4 为止，先写完正式代码再统一测试，没实测的假设列进架构文档 |
| [D20260918-main-03](D20260918-main-03-arch-update.md) | 2026-09-18 | 把原型 V1–V4 的结论写回架构文档 | 完成 | 用户变更改为实测结论（Hysteria2 需攒批）；splice 不发生；构建标签确定 |
| [D20260918-main-02](D20260918-main-02-vps-init.md) | 2026-09-18 | 填入 VPS 信息并初始化 | 完成 | VPS 就绪：内存 slice 7 GiB、目录、只读 Deploy Key、仓库和两个 worktree；机器上有管理员的其他服务，规范里补了约束 |
| [D20260918-main-01](D20260918-main-01-branches-and-vps-builds.md) | 2026-09-18 | 分支模型、提交规范、编译上 VPS | 完成 | 建立 main / panel / agent 三分支和文件归属规则；提交信息改中文、及时推送；编译和测试都放到 VPS 并强制预留 1 GiB 内存 |
| [D20260917-main-01](D20260917-main-01-project-setup-and-design.md) | 2026-09-17 | 项目初始化、调研与设计讨论 | 完成 | 建仓库和文档结构；研究妙妙屋 X；完成范围、架构、订阅、节点、面板设计；定测试 VPS 和记录规范；写原型计划。未写代码 |

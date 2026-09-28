# R20260927-agent-05 M7 复测：卸载时正确识别 systemd 已在停止服务

> 摘要：R20260927-agent-04 里 `systemctl stop --no-block` 被停止任务一起杀掉，被误报成失败。改为「被信号结束也算成功」后，用同一个脚本把 M7 全部复测一遍：卸载和「服务器已删除」都走「等 systemd 停止服务」，其他步骤照样全部通过。

- 状态：通过
- 日期：2026-09-27；执行：YUCHEN（Windows）/ Claude Code 会话
- 关联：[ExecPlan](../../exec-plans/completed/2026-09-23-agent.md) M7；上一轮 [R20260927-agent-04](R20260927-agent-04-upgrade.md)；开发轮次 D20260927-agent-05
- 代码：`agent-upgrade` `3e2ec44`
- 环境：同 R20260927-agent-04

## 目的

1. 卸载和「服务器已删除」时日志走「等 systemd 停止服务」，不再报 systemctl 失败
2. R20260927-agent-04 的其他步骤照样通过

## 做法

同 R20260927-agent-04：`agent/test/scripts/m7-upgrade.sh R20260927-agent-05 YUCHEN`。

## 结果

| 步骤 | 结果 |
| --- | --- |
| 没有公钥、SHA-256 不对、签名不对、404 | 都回失败，原因同上一轮；二进制、备份、标记不变 |
| 升到 0.0.2 | 回 ok 后补发流量上报，systemd 拉起 0.0.2，`.bak` 是 0.0.1，认证后删标记 |
| 升到 0.0.3（被拒） | 3 分钟后（08:15:41）换回，2 秒后 0.0.2 的 Hello 带 `rolledBackFrom: 0.0.3-3e2ec44`，认证后删标记 |
| 卸载 | 回 ok；日志「卸载完成，等 systemd 停止服务」；文件和 nftables 表删掉，服务 inactive |
| 服务器已删除 | 日志「主控说这台服务器已在面板上删除，卸载 Agent」「卸载完成，等 systemd 停止服务」；文件删掉，服务 inactive |

- Agent 日志里的错误只有测试故意造成的几次升级失败和试运行超时

## 结论

- M7 的运行验证全部通过，可以开 PR

## 下一步

开 PR 合并 M7。

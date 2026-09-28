# D20260918-main-03 把原型 V1–V4 的结论写回架构文档

> 摘要：agent 分支的原型验证出了结果，把其中影响设计的几条写回 main 的 architecture.md：用户变更怎么下发、Hysteria2 要攒批、splice 不发生、构建标签确定。

- 状态：完成
- 关联：agent 分支的[原型计划](https://github.com/yuuuki-creation/open-proxy/blob/agent/docs/exec-plans/completed/2026-09-17-agent-core-prototype.md)（V1–V4）；调试记录在 agent 分支
- 提交：本记录所在的提交

## 目的

设计文档属于 main，原型结论必须回流，否则 panel 分支和以后的会话看到的还是旧假设。

## 做了什么

- `docs/design-docs/architecture.md`：
  - 「用户变更」从「待验证」改为实测结论表：TCP 类协议重建入站无感（1 毫秒），Hysteria2 会断掉全部在传会话，Shadowsocks 可用 `UpdateUsers` 热更新
  - 补 Hysteria2 的规避办法：变更攒批、停用靠追踪层即时断流、加人尽量合并
  - 补两条实现注意事项：Shadowsocks 别开 `managed`；`box.New` 前要自己建好服务注册表，否则运行时重建入站会崩溃
  - 「将来的限速」改为实测结论：这套配置下不发生 splice，统计不会被绕过
  - 技术栈表补 Agent 构建标签，待调研项删掉已确认的那条
- `docs/status/main.md`：更新下一步

## 结果

main 上的设计文档和原型实测一致。

## 遗留问题

- V5–V7（Mieru、落地出口、端口跳跃）还没验证，相关段落仍是待验证状态

## 下一轮

等 agent 分支做完 V5–V7 再回流一次。

# R20260927-agent-08 禁止访问本机和内网的运行验证（技术债 agent-5）

> 摘要：还没跑。2026-09-27 准备好时测试锁被 R20260927-panel-web-01 占着，之后管理员要求收尾暂停。目的和做法已写好，下一次拿到测试锁后照做即可。

- 状态：未执行
- 日期：2026-09-27；执行：YUCHEN（Windows）/ Claude Code 会话
- 关联：[ExecPlan](../../exec-plans/active/2026-09-27-agent-guard.md)；技术债 agent-5；开发轮次 D20260927-agent-07
- 代码：`agent-guard`（跑的时候填提交号）
- 环境：测试 VPS，Debian 13；sing-box v1.14.1、mieru v3.38.0（客户端也是它们）

## 目的

1. 五种协议（Shadowsocks 2022、VLESS REALITY、Hysteria2、AnyTLS、Mieru）的 TCP 连接，目标是 127.0.0.1、localhost、<VPS_IP>（本机公网 IP）、10.0.0.1 时被拒绝，而且是马上被拒（不是等连接超时）；公网目标 example.com:80、1.1.1.1:80 正常
2. UDP：发往 127.0.0.1、localhost、<VPS_IP> 的包被丢掉；1.1.1.1:53 的 DNS 正常；同一个 UDP 会话里先发公网 DNS、再发 127.0.0.1，前者正常、后者被丢
3. 经落地出口：IP 字面量在节点就被拒；域名交给落地，落地（本机的自建落地）解析后再拒；公网目标正常
4. 回显服务在默认模式下收不到经代理来的连接和包；Agent 的调试日志里有拒绝记录；日志里没有终端转义字符
5. 带 `-allow-private-targets` 重启后：启动日志有警告，上面被拒的本机目标都能连通，没有拒绝日志

## 做法

脚本 `agent/test/scripts/guard.sh <run-id> <开发电脑名>`（自己加测试锁，锁被占用时退出码 3；结束时停进程、删表、删锁；端口只用 22000–22999，避开主控测试用的 21000–21999、28080、28443）：

- 假主控下发 6 个节点（端口 22001–22006；第 6 个是 Shadowsocks，经落地出口 1 到本机的自建落地 127.0.0.1:22100）；Agent 带 `-log-level debug`
- 回显服务（`proxyclient -echo-server`）监听 0.0.0.0:22997 的 TCP 和 UDP，测试专用表 `inet open_proxy_test` 里挡住从外网进来的包；先不经代理确认 127.0.0.1 和本机公网 IP 上都能连通（对照）
- 每个检查跑一次 `proxyclient`（等回复 3 秒），按期望（成功 / 失败）记进结果表
- 连一个公网上没开的端口，让 sing-box 自己打一条错误日志，检查 Agent 日志里的 ESC 字符
- 最后带 `-allow-private-targets` 重启 Agent，再测五种协议的本机目标

## 结果

（跑完补）

## 结论

（跑完补）

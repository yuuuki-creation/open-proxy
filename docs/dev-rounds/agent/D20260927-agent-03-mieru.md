# D20260927-agent-03 正式 Agent：M5 Mieru

> 摘要：Mieru 节点接进 Agent：每个节点一个 mieru mux，会话回 socks5 应答后交给 sing-box 路由，支持 CONNECT 和 UDP ASSOCIATE；增删用户热更新、改密码才重建。顺带修了「重置凭据后 Agent 不生效」。写了假主控和 Mieru 测试客户端，在测试 VPS 上两轮运行验证通过。经 PR #10 合并。

- 状态：完成
- 关联：[ExecPlan](../../exec-plans/completed/2026-09-23-agent.md) M5；调试记录：[R20260927-agent-01](../../debug-runs/agent/R20260927-agent-01-mieru.md)、[R20260927-agent-02](../../debug-runs/agent/R20260927-agent-02-mieru-close.md)
- 提交：工作分支 `agent-mieru`（`f5a8bae`..`aa3ee4d`）squash 合并为 `572e3fb`（PR #10）

## 目的

按计划 M5 实现 Mieru 节点：连接和其他协议一样经过追踪层和分发出站；用户变化时尽量不断线。

## 做了什么

- `agent/internal/mieru/`：`server.go`（mux 的启动、热更新用户、关闭）、`conn.go`（读 socks5 请求，CONNECT 用 sing 的 `LazyConn`，UDP ASSOCIATE 包装成 `N.PacketConn`）、`listener.go`（不设 SO_REUSEPORT、只收 IPv4、关实例时同步断开底层连接、Accept 临时错误重试）
- `agent/internal/core/`：Mieru 节点和 sing-box 入站按同一个 tag 管；`UpdateMieruUsers`；sing-box 日志关颜色码
- `agent/internal/state/apply.go`：节点用户连凭据一起比；Mieru 热更新、密码变了重建；Hysteria2 只删用户且没人改凭据才不重建
- `agent/cmd/op-agent/main.go`：退出时先等状态管理停下再关 sing-box
- `agent/test/`：假主控 `fakemaster`、Mieru 测试客户端 `mieruclient`、验证脚本 `scripts/m5-mieru.sh`
- 取舍和发现写在计划的「决策记录」「意外发现」（2026-09-27）

## 结果

- 测试 VPS 上 gofmt、go vet、staticcheck、两种架构编译通过；PR #10 的 CI 通过，已合并
- 运行验证见两条调试记录：功能全部符合预期；第一轮发现 mieru 关实例慢，改后复测通过

## 遗留问题

- 已验证的 Mieru 相关假设要由 main 更新 architecture.md「未验证的假设」；architecture.md「Agent 内部」写的是 apis/server，要改成 pkg/protocol（见计划的决策记录）
- 所有协议的节点都能访问服务器本机回环地址和内网（sing-box 默认不拦），记为技术债 agent-5

## 下一轮

M6：Hysteria2 端口跳跃（nftables）。

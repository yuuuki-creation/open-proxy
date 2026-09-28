# R20260927-agent-01 M5 Mieru 节点的运行验证

> 摘要：在测试 VPS 上用假主控下发 Mieru 节点，用 mieru 官方客户端库连它。CONNECT、UDP ASSOCIATE、按用户计量、热更新用户、改密码重建、删节点都符合预期；追踪层和分发出站对 Mieru 的连接照常生效。发现 mieru 关实例时逐个关会话、每个最多等 1 秒，改密码那次应用用了 1.07 秒，已改为自己断开底层连接、后台关 mux，复测见 R20260927-agent-02。

- 状态：部分通过（功能全部通过；关实例慢，修改后复测）
- 日期：2026-09-27；执行：YUCHEN（Windows）/ Claude Code 会话
- 关联：[ExecPlan](../../exec-plans/completed/2026-09-23-agent.md) M5；main 的 architecture.md「未验证的假设」；开发轮次 D20260927-agent-03
- 代码：`agent-mieru` `f226de1`
- 环境：测试 VPS，Debian 13，Go 1.26.5；mieru v3.38.0，sing-box v1.14.1

## 目的

M5 把 Mieru 节点接进了 Agent（不经过 apis/server，直接用 mieru 的 mux，见计划的决策记录）。确认：

1. 节点只监听 IPv4，监听不带 SO_REUSEPORT
2. CONNECT 和 UDP ASSOCIATE 都能经节点出去，流量按用户记到这个节点上
3. 只增删用户时热更新：已有连接不断，被删的用户被断开且连不上，新用户能用
4. 有用户改了密码时重建：旧密码建立的连接断开，旧密码连不上，新密码能用
5. 删掉节点后端口释放

## 做法

脚本 `agent/test/scripts/m5-mieru.sh`（在 VPS 上执行，自己加测试锁、结束清理）：

- 编译 op-agent 和两个测试工具：`agent/test/fakemaster`（假主控：回 HelloResult、推送期望状态、打印 Agent 发来的消息）、`agent/test/mieruclient`（用 mieru 的 apis/client 连节点，做 HTTP 访问、DNS 查询、长连接回显）
- 假主控监听 127.0.0.1:21000，Agent 的 `MASTER_URL` 指向它；Mieru 节点端口 21001，回显服务 127.0.0.1:21999；用户密码和 Token 每次随机生成
- 依次推送 4 份期望状态：用户 1、2 → 删 2 加 3 → 用户 1 改密码 → 删掉节点；每一步用客户端和 `ss` 检查，看假主控收到的 `StateReport`、`TrafficReport`

## 结果

| 检查 | 结果 |
| --- | --- |
| 监听 | 只有 `0.0.0.0:21001`（IPv4） |
| SO_REUSEPORT | 另一个设了 SO_REUSEPORT 的套接字绑同一端口失败（Address already in use），说明节点的监听没设 |
| 状态上报 | 4 次应用都没有失败项 |
| CONNECT | 2/2 成功（`generate_204` 回 204） |
| UDP ASSOCIATE | 2/2 成功（向 1.1.1.1 查 example.com，回答 2 条） |
| 错误密码 | 连不上（客户端 10 秒等不到应答） |
| 流量上报 | 节点 1 用户 1：上行 266、下行 414 字节，和两次 HTTP、两次 DNS 的量相符 |
| 删 2 加 3（热更新） | 用户 1 的回显 12/12 成功，一直是同一条连接；用户 2 的连接在应用的同一毫秒被断开；用户 3 能用，用户 2 新建连接连不上 |
| 用户 1 改密码（重建） | 旧密码的连接断开；新密码能用，旧密码连不上；这次应用用了 1.069 秒（其他几次 0–2 毫秒） |
| 删节点 | 端口释放 |

- 被删的用户 2 断开后，客户端在它原有的底层连接上又开了新会话，mieru 照样放行（它只在建底层连接时认证），被分发出站以「用户 2 不在节点 node-1 的放行名单里」拒绝
- sing-box 自己的日志带终端颜色码，journald 里显示成乱码

## 结论

- main 的 architecture.md「未验证的假设」里这几项对 Mieru 成立：服务端连接带用户身份、能按用户统计；连接交给 `RouteConnectionEx` 后追踪层和分发出站照常生效；删除用户后追踪层能断开他的存量连接；分发出站能从上下文取到入站 tag。需要由 main 更新该表
- 被删的用户还能在已有的底层连接上开会话，靠追踪层和分发出站拦住；改密码时必须重建（已这样做）
- 关实例慢：mieru 关 mux 时逐个关会话，每个会话最多等 1 秒把关闭请求发出去，底层连接已断时一定等满（v3.38.0 `Session.closeWithError`）。会话多时会卡住状态管理和退出流程。改为：自己同步关掉监听和所有底层 TCP 连接，mux 放到后台关
- sing-box 的日志关掉颜色码

## 下一步

用修改后的代码复测（R20260927-agent-02）。

# 验证 Agent 代理内核方案（内嵌 sing-box 和 Mieru）

> 摘要：已结束。V1–V4 全部通过：计量准确、停用能即时断流；增删用户时 TCP 类协议重建入站无感，Hysteria2 会断全部在传会话。V5–V7 不在原型里做，等正式代码写完后统一测试（2026-09-23 决定）。

- 状态：已完成（V1–V4；V5–V7 转到正式代码写完后统一测试）
- 分支：agent
- 创建：2026-09-17
- 最后更新：2026-09-23

## 目标与背景

Agent 的设计（main 分支 `docs/design-docs/architecture.md` 的「Agent 内部」）建立在几个还没验证过的假设上：

- 可以把 sing-box 1.14.1 当 Go 库内嵌，用代码构造配置并启动
- 通过 sing-box 的 `ConnectionTracker` 能按「用户 × 入站」准确统计流量，包括 VLESS Vision 走零拷贝（splice）的情况
- 能通过追踪层主动断开某个用户的全部连接
- 按同一个 tag 重建入站来增删用户时，其他人的已有连接能保持
- Mieru 的服务端库可以多用户、按用户统计、换实例时不断开旧连接
- 按同一个 tag 替换落地出口后，新连接立即走新出口
- Hysteria2 端口跳跃可以用 nftables 幂等地建立和清理

这份计划做一个原型，逐项回答这些问题。结论决定正式 Agent 怎么实现；如果关键假设不成立（例如 Vision 的流量统计不准且无法修正，或者重建入站必然断开所有人），要回到设计讨论。

## 进度

- [x] (2026-09-18) 测试 VPS 就绪：连接信息已填、只读 Deploy Key 已加；内存 slice（7 GiB）、目录、worktree 就绪（[R20260918-main-01](../../debug-runs/main/R20260918-main-01-vps-init.md)）
- [x] (2026-09-18) 建立 `prototypes/agent-core/` Go 模块，锁定 sing-box v1.14.1（mieru 还没用到，`go mod tidy` 后不在 `go.mod` 里）；构建标签为 `with_quic,with_utls`；在 VPS 上编译通过
- [x] (2026-09-18) V1 四种入站连通（[R20260918-agent-01](../../debug-runs/agent/R20260918-agent-01-v1-connectivity.md)）
- [x] (2026-09-18) V2 按用户统计的准确性：TCP 下行 +0.325%、上行 +0.000%、UDP 双向 +0.000%（[R20260918-agent-01](../../debug-runs/agent/R20260918-agent-01-v1-connectivity.md)、[R20260918-agent-02](../../debug-runs/agent/R20260918-agent-02-v2-udp-and-v3.md)）
- [x] (2026-09-18) V3 停用用户并断开连接（[R20260918-agent-02](../../debug-runs/agent/R20260918-agent-02-v2-udp-and-v3.md)）
- [x] (2026-09-18) V4 重建入站增删用户时其他人是否断线（[R20260918-agent-03](../../debug-runs/agent/R20260918-agent-03-v4-user-changes.md)）
- [ ] V5 Mieru 内嵌（不在原型里做，见决策记录 2026-09-23）
- [ ] V6 替换落地出口（同上）
- [ ] V7 Hysteria2 端口跳跃（同上）
- [x] (2026-09-23) 结论写回 main 分支的 architecture.md（D20260918-main-03、D20260923-main-01），填写「结果与复盘」

每完成一项，在这一行后面链接对应的调试记录（`docs/debug-runs/agent/`）。

## 意外发现

- 发现：这套配置下 sing-box **根本不走 splice 零拷贝**，挂不挂追踪层都一样。原因推测是 REALITY 的外层 TLS 由服务端终结，服务端拿到的不是裸 TCP 连接，splice 的前提（两端都是裸 TCP）不成立
  证据：`strace -f -e trace=splice -c` 在 100 MB HTTPS 传输期间统计到 0 次 splice，两种情况都是（[R20260918-agent-01](../../debug-runs/agent/R20260918-agent-01-v1-connectivity.md)）
  影响：Xray Vision 那套「零拷贝绕过统计」的担心在 sing-box 这条路上不存在；将来加限速包装也不会因此丢掉已有的零拷贝
- 发现：REALITY 服务端需要 `with_utls` 构建标签，旧的 `with_reality_server` 已合并进去，继续使用会故意编译失败
  证据：`common/tls/reality_stub.go` 里写明了这件事
- 发现：Hysteria2（QUIC）重建入站会打断该入站上**全部**在传会话，TCP 类协议（VLESS、AnyTLS、Shadowsocks）则完全无感
  证据：每 2 秒采样的计数增量，TCP 类始终约 6 MB，Hysteria2 重建后直接归零且不恢复；重建后新连接立刻可用（[R20260918-agent-03](../../debug-runs/agent/R20260918-agent-03-v4-user-changes.md)）
  影响：正式实现要把短时间内的多次用户变更合并成一次重建；停用用户靠追踪层即时断流，不必立刻重建
- 发现：Shadowsocks 可以用 `ManagedSSMServer.UpdateUsers` 热更新用户，不用重建入站，0 毫秒且不影响已有连接；注意不要开 `managed` 选项（那会禁止配置里的静态用户列表）
  证据：同上记录
- 发现：`box.New` 把服务注册进 context 的服务注册表，注册表存在则复用。必须自己先建好注册表和 pause 管理器再传给 `box.New`，否则运行时调用 `InboundManager.Create` 会空指针崩溃
  证据：崩溃栈指向 `dialer.NewWithOptions`，改了 context 准备顺序后重建正常（1 毫秒）
- 发现：测断流这类时序，必须让**服务端**按速率发送，不能用客户端限速
  证据：用 `curl --limit-rate` 时，服务端早已全速推完数据，客户端只是在读自己的缓冲，测出「停用后还传了 30 秒」的假结果；改成服务端限速后，实测停用后只多收一个缓冲区（约 131 KB）（[R20260918-agent-02](../../debug-runs/agent/R20260918-agent-02-v2-udp-and-v3.md)）
  影响：以后所有时序测量以服务端计数为准，不看客户端耗时

## 决策记录

- 决策：原型代码放在 agent 分支的 `prototypes/agent-core/`，不作为正式 Agent 代码
  理由：验证过程可追溯，写正式 Agent 时可以参考
  日期：2026-09-17
- 决策：编译和验证全部在测试 VPS 上做，不在开发电脑上跑
  理由：开发电脑是 Windows，没有 splice 和 nftables；统一环境也避免结论不可比
  日期：2026-09-18
- 决策：原型到 V4 为止。V5–V7，以及 V3、V4 没覆盖到的几项，不在原型里做，等正式代码写完后在测试 VPS 上统一测试
  理由：管理员决定先把正式代码写完再测。待测的假设统一列在 main 的 `architecture.md`「未验证的假设」
  日期：2026-09-23

## 结果与复盘

2026-09-23 结束：V1–V4 通过，V5–V7 没做。

- 实现了：内嵌 sing-box 跑通四种入站；按「入站 × 用户」计量，误差在 1% 以内；停用用户一个缓冲区内断流；摸清了增删用户时各协议的行为。数据见各调试记录
- 和最初目标相比：核心假设（内嵌、追踪层计量、断连接、同 tag 重建入站）都成立，设计不用推翻。Mieru、落地出口、端口跳跃，以及 V3、V4 没覆盖的几项还没验证，清单在 main 的 `architecture.md`「未验证的假设」
- 写正式 Agent 时要避开的两个坑（原型里有，原型不再改）：
  - `tracker.go` 里「检查是否已停用」和「登记连接」不在同一把锁里，恰好在停用那一刻建立的连接会漏断
  - `server.go` 的增删用户改完名单就放锁，再去重建入站；并发变更会交错，甚至让旧名单最后生效
  两条都已写进 `architecture.md` 的「Agent 内部」
- 学到的：测时序要让服务端限速；测试脚本要进仓库（这次的 `v4.py` 等只留在 VPS 的 run 目录，见技术债 agent-1）

## 现状与上下文

- 本分支还没有代码。相关设计在 main 分支：`docs/design-docs/architecture.md`（Agent 内部、落地出口、用户变更）、`docs/design-docs/nodes.md`（各协议参数、端口跳跃）
- 术语：
  - **入站**：sing-box 里监听端口、接受客户端连接的配置单元，用 tag 标识
  - **ConnectionTracker**：sing-box 的接口，通过 `Router.AppendTracker` 注册。sing-box 选好出站之后、拨号之前调用它，参数里带入站 tag 和用户，可以返回包装过的连接
  - **Vision**：VLESS 的流控模式，发现内层是 TLS 时切换成直接拷贝；在 Linux 上可能用 splice 在内核里搬数据，绕过应用层的包装
- 已知接口（来自 main 分支 `reference/mmwx/code-agent-xray.md` 第 11 节，是读 v1.14.1 源码的结论，未验证）：
  - `adapter.Router.AppendTracker(ConnectionTracker)`；`ConnectionTracker` 有 `RoutedConnection`、`RoutedPacketConnection`、`RoutedFlow` 三个方法
  - `adapter.InboundManager` 有 `Get`、`Remove`、`Create`
  - Shadowsocks 有运行时更新用户的 `ManagedSSMServer.UpdateUsers`
  - sing 库的拷贝函数会一层层剥掉实现了特定接口的包装，最后可能直接 splice
- Mieru 服务端库 `apis/server`：`Store(配置)` → `Start()` → 循环 `Accept()`，返回的连接带用户身份；配置只能在 `Start()` 前写入；`Stop()` 不断开已建立的连接
- 许可证：原型引用 GPL-3.0 代码，按 GPL-3.0 对待；不复制妙妙屋 X 的代码

## 实施方案

1. **原型程序**：`prototypes/agent-core/`，程序名 `opcore`
   - 用 Go 代码构造 sing-box 配置（option 结构体），内嵌启动一个实例
   - 注册自己的 ConnectionTracker：按（入站 tag, 用户）计数；按用户登记每条连接的关闭函数
   - 内嵌一个 Mieru 服务端，`Accept` 后自己拨号转发，使用同一套计数器
   - 提供一个只监听 127.0.0.1 的控制接口，用来增删用户（同 tag 重建入站）、停用用户、替换出口、读取统计
2. **测试驱动**（都在 VPS 上）：
   - 客户端：sing-box 命令行（VLESS + REALITY、Hysteria2、AnyTLS、SS 2022）、mihomo（Mieru），每个客户端开一个本地 SOCKS 入口
   - 流量源：原型内置的 HTTP 和 HTTPS 服务，提供固定大小的下载和上传。Vision 只有内层是 TLS 时才切换直接拷贝，所以 V2 的 Vision 测试必须用 HTTPS 流量源
   - 长连接：保持一个持续传输的下载和一个空闲连接，用来观察重建入站时是否断开
3. **每轮流程**：开发电脑改代码 → 提交推送 → VPS 拉取该提交 → 在 `open-proxy.slice` 里编译 → 持锁跑测试 → 写调试记录 → 清理

## 具体步骤

| 验证项 | 步骤 |
| --- | --- |
| V1 | 构造四种入站并启动；每种客户端下载 10 MB 文件，校验哈希 |
| V2 | 每种协议分别下载、上传 100 MB，比较客户端实际传输字节数和统计值；UDP 用自建 UDP 回显发送固定字节数；VLESS Vision 用 HTTPS 流量源单独测一遍 |
| V3 | 用户 A、B 各保持一个长下载；停用 A；观察 A 的连接是否在 1 秒内断开、重连是否被拒，B 是否不受影响 |
| V4 | A、B 各保持长下载和空闲连接；新增用户 C（同 tag 重建入站），再删除 C；分别记录 TCP 类（VLESS、AnyTLS、SS 2022）和 Hysteria2 上 A、B 的连接是否断开、断开多久；SS 2022 另测 `UpdateUsers` |
| V5 | Mieru 两个用户分别下载，核对统计；`Stop()` 后新建实例接管同一端口，记录端口能否立即使用、旧连接是否继续传输 |
| V6 | 在 VPS 上起两个带认证的 SOCKS5 服务当出口 A、B；节点出口先指向 A，同 tag 替换为 B；检查新连接走 B，记录旧连接的表现 |
| V7 | 用 nftables 表 `open_proxy_test` 把 UDP 30000–30099 转发到 Hysteria2 端口；客户端用端口范围连通；重复应用规则不产生重复条目；删除节点后规则清空；原型重启后规则幂等重建 |

## 验证与验收

| 验证项 | 通过标准 |
| --- | --- |
| V1 | 四种协议的客户端都能下载，哈希一致 |
| V2 | 统计值和实际字节数误差在 1% 以内，TCP、UDP、Vision 都满足 |
| V3 | 被停用用户的连接 1 秒内断开，新连接被拒；其他用户不受影响 |
| V4 | 记录清楚每类入站的行为。理想结果：其他人的已有连接不断开 |
| V5 | 统计误差在 1% 以内；换实例时旧连接不断开 |
| V6 | 替换后新连接走新出口 |
| V7 | 客户端通过端口范围连通；规则幂等，清理干净 |

V4、V6 属于摸清行为，结果不理想也不算失败，但要写进「意外发现」，并决定正式 Agent 的应对方式。

## 可重复执行与回滚

- 每轮调试使用独立的 run 目录，互不影响
- 除了要从外部连的测试入站，其他监听都绑 127.0.0.1
- 原型不修改系统配置，唯一例外是 V7 的 nftables 表，每轮结束执行 `nft delete table inet open_proxy_test` 清理
- 所有进程用 `systemd-run` 在 `open-proxy.slice` 里启动，结束后停止；不部署常驻服务

## 接口与依赖

- Go（装在 VPS 的 `/opt/open-proxy/toolchains/`，版本写进调试记录）
- `github.com/sagernet/sing-box` v1.14.1。构建标签：`with_quic`（Hysteria2）、`with_utls`（REALITY 服务端也靠它）
- `github.com/enfein/mieru/v3` v3.37.0 的 `apis/server`
- 测试客户端：sing-box 1.14.1 命令行、mihomo 最新稳定版；实际使用的版本写进调试记录

# 禁止代理用户访问节点服务器本机和内网（技术债 agent-5）

> 摘要：进行中，2026-09-27 管理员要求收尾暂停。代码写完并推送到工作分支 `agent-guard`（PR #18，未合并）：分发出站在拨号前检查目标，直连的域名先解析、过滤后按剩下的地址拨号，UDP 每个包都查；测试用参数 `-allow-private-targets` 也加了。还没做：测试 VPS 上的运行验证（当时测试锁被 R20260927-panel-web-01 占着）、合并、main 上 architecture.md 补这个参数。agent-4 这轮不做。

- 状态：进行中（暂停）
- 分支：agent（工作分支 `agent-guard`，PR #18）
- 创建：2026-09-27
- 最后更新：2026-09-27

## 目标与背景

技术债 agent-5：sing-box 默认不拦目标地址，代理用户能经节点连到节点服务器本机（只监听 127.0.0.1 的服务、本机公网 IP 上的端口）和内网。调试记录 R20260927-agent-01 里经 Mieru 节点连到了 127.0.0.1:21999。做完后，五种协议（Shadowsocks 2022、VLESS REALITY、Hysteria2、AnyTLS、Mieru）和自建落地的连接都不能访问：回环、私有（RFC 1918、fc00::/7）、链路本地、未指定、组播、100.64.0.0/10，以及本机网卡上的任何地址；TCP 和 UDP 都管。面板和期望状态里没有开关；只有测试用的命令行参数 `-allow-private-targets` 能关掉检查（测试机上能连的目标都在本机，主控的端到端测试要用）。设计结论已写进 main 的 architecture.md「不能访问服务器本机和内网」（main `3519e78`）。

同一轮顺带：sing-box 日志去掉终端颜色码；判断 agent-4（重启后失败项不保持原样）能不能做。

## 进度

- [x] (2026-09-27) 读 sing-box v1.14.1 的路由和连接管理：出站实现 `adapter.ConnectionHandler` / `PacketConnectionHandler` 时，路由直接把连接交给它（`route/route.go` 的 `routeConnection`），可以在拨号前检查；连接管理看到 `DestinationAddresses` 就按这些地址拨号，不再解析
- [x] (2026-09-27) 代码：`agent/internal/core/guard.go`（地址判断、本机地址缓存、解析过滤、拒绝日志限流、逐包检查的 UDP 连接）；`dispatch.go` 实现两个接口；`core.Options` 和命令行参数 `-allow-private-targets`（开了就不检查，启动时打警告）；`cmd/op-agent` 加 `-log-level`
- [x] (2026-09-27) 测试工具 `agent/test/proxyclient`（内嵌 sing-box 当四种协议的客户端，加 Mieru）和脚本 `agent/test/scripts/guard.sh`（默认模式：五种协议加经落地出口的节点，TCP / UDP 的禁止目标和公网目标；再带 `-allow-private-targets` 重启，禁止目标应放行）
- [x] (2026-09-27) 测试 VPS 上编译和静态检查：提交 `9ef2144`（还没有 `-allow-private-targets`）全部通过；之后的提交见下一项
- [x] (2026-09-27) 开 PR #18（CI 跑的是 `9ef2144`）
- [ ] 最后一个提交（加了 `-allow-private-targets`）在 VPS 上的编译和静态检查
- [ ] 运行验证：`bash agent/test/scripts/guard.sh <run-id> <开发电脑名>`，run-id 用 R20260927-agent-08（调试记录已写好目的和做法）；要先拿到测试锁（当时被 R20260927-panel-web-01 占着）。端口只用 22000–22999（21000–21999、28080、28443 是主控测试的）
- [ ] PR #18 的 CI 通过、运行验证通过后 squash 合并；把 PR 正文的「验证」补上结果表
- [ ] main 的 architecture.md「不能访问服务器本机和内网」里「不设开关，一律禁止」改成：面板和期望状态里没有开关，只有测试用的命令行参数 `-allow-private-targets`（启动时打警告，安装脚本经 `OP_AGENT_ARGS` 传）。属于 main 的文件，在 main 上改
- [ ] 收尾：调试记录补结果，开发轮次记录 D20260927-agent-07（agent 分支上已写，状态「部分完成」）改成完成，技术债 agent-5 改「已解决」，计划移到 completed

## 意外发现

- 发现：sing-box 日志的颜色码在 M5 已经关掉了（`core.Start` 设了 `DisableColor`，PR #10 `572e3fb`）；sing-box 的日志只经过实例自己的日志工厂，全局的 `log.StdLogger()` 在我们用到的代码里没人调用
  证据：sing-box v1.14.1 `log/log.go`（`DisableColors: logOptions.DisableColor`）、`log/format.go`；在 sing-box 源码里搜 `log.StdLogger()` 和包级的 `log.Error(` 等
  影响：这一项不用改代码，`guard.sh` 里有一步检查 Agent 日志里的 ESC 字符，运行验证时确认。主会话看到的颜色码可能来自 `572e3fb` 之前编译的 Agent
- 发现：测试 VPS 上主会话用另一个 worktree `/opt/open-proxy/src/agent-e2e` 编译 Agent；本计划的编译检查用 `/opt/open-proxy/src/agent`，互不影响
  证据：VPS 上 `git worktree list`
  影响：无

## 决策记录

- 决策：检查放在分发出站，分发出站实现 `adapter.ConnectionHandler` 和 `adapter.PacketConnectionHandler`：拨号前检查放行名单和目标，拒绝的按失败关掉（`N.CloseOnHandshakeFailure`），通过的交给 sing-box 的连接管理
  理由：所有入站（包括 Mieru 和自建落地）的连接都经过分发出站，一处覆盖全部；在拨号前拒绝，sing-box 的连接管理就不会为每个被拒的连接打一条错误日志
  日期：2026-09-27
- 决策：直连、目标是域名时，用 sing-box 的 DNS（和直连出站自己解析时一样）解析一次，去掉不允许的地址，IPv4 排前面，填进 `DestinationAddresses`；连接管理按这些地址逐个经 `DialContext` / `ListenPacket` 拨号，不会再解析。`DialContext`、`ListenPacket` 里再查一遍（兜底别的调用路径）
  理由：防 DNS 重绑定；复用 sing-box 自己的机制，UDP 第一个包是域名时它还会把回包地址换回域名
  日期：2026-09-27
- 决策：UDP 每个包都查：包在 `guardedPacketConn` 里，发往不允许地址的包丢掉、会话照常；直连时包的目标是域名的，解析后发往允许的地址（每个会话缓存一分钟）。第一个包的目标不允许时整个会话按失败处理。包装只实现 `N.NetPacketConn` 和头尾余量接口，不暴露上游
  理由：一个 UDP 会话可以发往不同目标；sing 的拷贝循环会顺着 `Upstream` 等接口找到最里层直接写，那样会绕过检查；余量要照实报，否则经落地出口（SOCKS5 要在包前加头）时会越界
  日期：2026-09-27
- 决策：除了要求的几类，还拦 0.0.0.0/8（Linux 上连它等于连本机）和 255.255.255.255；地址先去掉 IPv4 映射（`::ffff:127.0.0.1` 当 127.0.0.1）。本机地址用 `net.InterfaceAddrs` 读，缓存一分钟；读失败时沿用上一次的结果
  理由：这两个网段和映射地址都能绕到本机；网卡地址变化不频繁
  日期：2026-09-27
- 决策：拒绝日志用调试级别，每 10 秒最多逐条记 10 条，多出来的只记总数；新加 `-log-level` 参数（默认 info），测试时用 debug 看拒绝日志
  理由：按要求别刷屏；原来没有办法打开调试日志
  日期：2026-09-27
- 决策：走落地出口时只检查 IP 字面量，域名交给落地解析；Agent 连落地服务器本身（出站的服务器地址）不经过分发出站，不受限制。自建落地（本机当落地）的连接走直连，照样全部检查
  理由：按要求；落地可能就在本机
  日期：2026-09-27
- 决策：测试用参数 `-allow-private-targets`（默认关）：开了就完全跳过检查（域名照常由出站自己解析，UDP 连接不包装），启动时打一条警告日志；只能在命令行上加，面板和期望状态里都没有
  理由：主会话的要求：测试 VPS 上所有目标都在本机，修完 agent-5 以后主控的端到端测试就没有能连的目标了
  日期：2026-09-27
- 决策：sing-box 日志的颜色码不改代码，只在运行验证时确认（见「意外发现」）
  理由：M5 已经关掉
  日期：2026-09-27
- 决策：agent-4 这轮不做，留在技术债（原因和建议做法写在 `docs/exec-plans/tech-debt/agent.md`）
  理由：要新增本地文件（记下每一项实际在跑的配置，启动时先按它恢复再应用最新的期望状态），main 的 architecture.md「Agent 的文件」要先改；要动状态管理的核心逻辑（每一项在成功、保持原样、失败恢复、热更新几种情况下各自该记什么），还要处理恢复时放行名单的短暂窗口；需要单独的重启测试。和安全修复放在一个 PR 里风险大
  日期：2026-09-27

## 结果与复盘

（完成后填写）

## 验证与验收

- 测试 VPS 上 gofmt、go vet、staticcheck、linux amd64 / arm64 编译通过；PR 上 `agent.yml` 通过
- `agent/test/scripts/guard.sh` 的结果表里期望和实际全部一致：
  - 默认模式：五种协议各测 TCP 连 127.0.0.1、localhost、<VPS_IP>、10.0.0.1 上的端口都被拒绝（耗时应该很短，不是等连接超时），UDP 发往 127.0.0.1、localhost、<VPS_IP> 被丢掉，同一 UDP 会话里先发公网 DNS（正常）再发 127.0.0.1（丢掉）；公网的 example.com:80、1.1.1.1:80、1.1.1.1:53 正常；经落地出口时 IP 字面量在节点被拒、域名到落地再被拒；回显服务在默认模式下收不到经代理来的连接和包；Agent 日志里没有终端转义字符
  - 带 `-allow-private-targets`：启动日志有警告；五种协议连 127.0.0.1、localhost、<VPS_IP> 的 TCP 和 127.0.0.1 的 UDP 都成功，没有拒绝日志

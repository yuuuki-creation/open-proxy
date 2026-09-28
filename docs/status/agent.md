# agent 分支状态

> 新会话先读 `AGENTS.md` 和本文件，通常就够开始工作。本文件不超过 40 行，每轮开发结束时**整篇重写**，不要在末尾追加。

- 更新：2026-09-27，[D20260927-agent-06](../dev-rounds/agent/D20260927-agent-06-reality.md)
- 本分支内容：正式 Agent `agent/`（Go，程序名 `op-agent`，内嵌 sing-box 和 Mieru）、原型 `prototypes/`

## 当前阶段

- 正式 Agent 的[计划](../exec-plans/completed/2026-09-23-agent.md)已完成：8 个阶段都已合并（PR #3、#5、#6、#8、#10、#12、#15、#16，另有修技术债 agent-3 的 #7），「结果与复盘」已填
- 每个阶段都在测试 VPS 上做过运行验证（调试记录 R20260927-agent-01 到 07）；各协议真实客户端的互通、落地出口、端口跳跃的真实转发、真实网段扫描、arm64 实机还没测

## 下一步

1. 另写计划，在测试 VPS 上做集成测试，覆盖 main 的 architecture.md「未验证的假设」剩下的几项（技术债 agent-2）；可以接着用 `agent/test/` 的假主控，主控写好后换成真的
2. 需要 main 处理（本分支不改 main 的文件）：architecture.md 里 Mieru 的做法（改用 mux，`Stop()` 会断连接）和「未验证的假设」里已验证的几项；nodes.md「没有 nftables 时用 iptables」不做了；code-style.md 目录表补 `internal/reality/`、`agent/test/`
3. 技术债 agent-4（重启后失败项不再保持原样）、agent-5（节点能访问服务器本机和内网，安全相关）、agent-6（新版本读升级标记前就崩溃时回滚不了）待用户定办法
4. 发布流程（编译时注入版本号和升级验签公钥、离线私钥签名）等主控的打包方案定下来再做

## 阻塞

- 无

## 本分支要点

- 改代码前读：计划的「决策记录」；main 的 `architecture.md`「Agent 内部」、`protocol.md`、`docs/code-style.md`
- 测试 VPS：`/opt/open-proxy/env.sh` 设好 Go 的路径和缓存；worktree `/opt/open-proxy/src/agent` 用 `git checkout --detach origin/<工作分支>` 切到要测的提交；远端脚本里会读标准输入的命令要加 `</dev/null`；已装 nftables 包（只用 `nft` 读规则，服务未启用）
- 运行验证：`agent/test/fakemaster`（假主控：推期望状态、发升级卸载和 REALITY 请求、测试 TLS 服务）、`agent/test/mieruclient`、`agent/test/scripts/m5-*.sh` 到 `m8-*.sh`（每个脚本自己加测试锁、结束清理）；VPS 上 Agent 要带 `-nft-table open_proxy_test`
- 升级要在编译时注入验签公钥：`-ldflags "-X main.version=<版本> -X main.upgradePublicKey=<base64 公钥>"`；不注入就不允许升级
- protobuf 代码：改了 `protocol/` 后在 `agent/` 下执行 `buf generate`（buf v1.73.0），生成物一起提交；CI 会比对
- Windows 开发电脑上 `gofmt -l` 会把 CRLF 换行的文件都报出来，那是签出时转的换行符；以 VPS 和 CI 为准
- 依赖版本：`google.golang.org/protobuf` 固定 v1.36.11；sing-box v1.14.1（构建标签 `with_quic,with_utls`）；mieru v3.38.0；google/nftables 沿用依赖图里的版本
- 技术债见 `docs/exec-plans/tech-debt/agent.md`：agent-2、agent-4、agent-5、agent-6

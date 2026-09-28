# agent 分支状态

> 新会话先读 `AGENTS.md` 和本文件，通常就够开始工作。本文件不超过 40 行，每轮开发结束时**整篇重写**，不要在末尾追加。

- 更新：2026-09-27，[D20260927-agent-07](../dev-rounds/agent/D20260927-agent-07-guard.md)
- 本分支内容：正式 Agent `agent/`（Go，程序名 `op-agent`，内嵌 sing-box 和 Mieru）、原型 `prototypes/`

## 当前阶段

- 正式 Agent 的 8 个阶段已完成（[计划](../exec-plans/completed/2026-09-23-agent.md)）
- 正在修技术债 agent-5（代理用户不能访问节点服务器本机和内网），**2026-09-27 管理员要求暂停**：代码和测试用参数 `-allow-private-targets` 在工作分支 `agent-guard`（最后提交 `a268126`，PR #18 已转为草稿，未合并）；计划在该分支的 `docs/exec-plans/active/2026-09-27-agent-guard.md`
- 每个改动都先在测试 VPS 上编译、静态检查和运行验证，再开 PR、CI 通过后 squash 合并

## 下一步

1. agent-5 继续：`a268126` 在 VPS 上编译检查；拿到测试锁后跑 `bash agent/test/scripts/guard.sh R20260927-agent-08 <开发电脑名>`（端口只用 22000–22999），结果补进调试记录；通过后 PR #18 转回正式、CI 通过后合并；按计划收尾
2. main 的 architecture.md「不能访问服务器本机和内网」把「不设开关，一律禁止」改成只有测试用的命令行参数 `-allow-private-targets` 能关（在 main 上改，或交给主会话）
3. agent-4（重启后失败项不再保持原样）这轮评估后不做，建议做法写在技术债里；agent-6 没动
4. 之后另写计划，在测试 VPS 上做集成测试（技术债 agent-2）

## 阻塞

- 测试 VPS 的测试锁：主会话在用时（锁的内容是 R20260927-panel-…），隔几分钟再看，不要清别人的锁、不要循环轮询

## 本分支要点

- 测试 VPS：`/opt/open-proxy/env.sh` 设好 Go 的路径和缓存；编译检查用 worktree `/opt/open-proxy/src/agent`（`git checkout --detach origin/<工作分支>`），主会话用 `src/agent-e2e`；远端脚本里会读标准输入的命令要加 `</dev/null`；已装 nftables 包（只用 `nft` 读规则）
- 端口：主控测试用 21000–21999、28080、28443，Agent 的测试用 22000–27999 或 30000–30999
- 运行验证工具：`agent/test/fakemaster`（假主控）、`agent/test/mieruclient`；`agent-guard` 分支上另有 `agent/test/proxyclient`（五种协议的客户端）；`agent/test/scripts/`（每个脚本自己加测试锁、结束清理）；VPS 上 Agent 要带 `-nft-table open_proxy_test`
- 升级要在编译时注入验签公钥：`-ldflags "-X main.version=<版本> -X main.upgradePublicKey=<base64 公钥>"`；不注入就不允许升级
- protobuf 代码：改了 `protocol/` 后在 `agent/` 下执行 `buf generate`（buf v1.73.0），生成物一起提交；CI 会比对
- 依赖版本：`google.golang.org/protobuf` 固定 v1.36.11；sing-box v1.14.1（构建标签 `with_quic,with_utls`）；mieru v3.38.0；google/nftables 沿用依赖图里的版本
- 技术债见 `docs/exec-plans/tech-debt/agent.md`：agent-2、agent-4、agent-5（进行中）、agent-6

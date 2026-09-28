# R20260927-agent-04 M7 升级、回滚、卸载的运行验证

> 摘要：在测试 VPS 上模拟安装 Agent（systemd 临时服务，Restart=always），用假主控发 Upgrade、Uninstall 和「服务器已删除」。没有公钥时拒绝升级、三种坏的升级什么都不变、升级后由 systemd 拉起新版本、新版本连不上时 3 分钟后换回旧版本并在 Hello 里报告、卸载删掉文件和 nftables 表并停掉服务，全部符合预期。发现 `systemctl stop --no-block` 自己也会被停止任务杀掉，被误报成失败，改后复测见 R20260927-agent-05。

- 状态：部分通过（功能全部符合预期；卸载时把 systemctl 被杀误报成失败，已改）
- 日期：2026-09-27；执行：YUCHEN（Windows）/ Claude Code 会话
- 关联：[ExecPlan](../../exec-plans/completed/2026-09-23-agent.md) M7；开发轮次 D20260927-agent-05
- 代码：`agent-upgrade` `953c8b9`
- 环境：测试 VPS，Debian 13，systemd 257

## 目的

1. 编译时没有内置公钥的 Agent 拒绝升级，回失败原因
2. SHA-256 不对、签名不对、下载不到时拒绝升级，二进制和升级标记都不变
3. 升级成功：回 `UpgradeResult` 后补发一次 `TrafficReport` 再退出，systemd 拉起新版本；旧版本留作 `.bak`；新版本认证成功后删掉升级标记
4. 新版本 3 分钟内没有通过认证：换回 `.bak` 并退出，systemd 拉起旧版本，旧版本的 Hello 带 `rolled_back_from`，认证后删标记
5. `Uninstall`：回复后补发流量上报，删 nftables 表、二进制、备份、配置、数据，服务被停掉且不再拉起
6. `HelloResult` 回「服务器已删除」时同样卸载

## 做法

脚本 `agent/test/scripts/m7-upgrade.sh`（自己加测试锁、结束清理）：用 `fakemaster -keygen` 生成一对 Ed25519 密钥，编译 3 个带公钥的版本（`0.0.1-<提交>`、`0.0.2-…`、`0.0.3-…`）和 1 个不带公钥的版本；二进制、配置、数据目录放在结果目录的 `inst/` 下，Agent 用 `systemd-run -p Restart=always` 起成临时服务。假主控提供二进制下载（检查 `Authorization: Bearer <Token>`）、用私钥签名后发 `Upgrade`，并拒绝 0.0.3 版本的 Hello（模拟新版本连不上）。依次：没有公钥的版本升级 → 装 0.0.1 → 三种坏的升级 → 升到 0.0.2 → 升到 0.0.3 并等试运行超时 → 卸载 → 重装后让假主控回「服务器已删除」。

## 结果

| 步骤 | 结果 |
| --- | --- |
| 没有公钥 | `UpgradeResult` 失败：「这个 Agent 编译时没有内置升级验签公钥，不允许升级」 |
| SHA-256 不对 / 签名不对 / 404 | 分别回「下载的文件和 SHA-256 对不上」「签名验证不通过」「下载新版本: 主控回复 404 Not Found」；二进制还是 0.0.1，没有 `.bak`，没有升级标记 |
| 升到 0.0.2 | 08:06:39.217 回 ok，1 毫秒后补发 `TrafficReport`；2 秒后（RestartSec）0.0.2 发 Hello；`.bak` 是 0.0.1；认证后升级标记删掉。下载 57 MB、校验、试跑共约 1 秒 |
| 升到 0.0.3（被拒） | 0.0.3 启动后 Hello 被拒；升级标记 `starts` 为 1；3 分钟后（08:09:50）换回旧版本并退出；2 秒后 0.0.2 的 Hello 带 `rolledBackFrom: 0.0.3-…`，认证后删标记 |
| 卸载 | 回 ok，同一毫秒补发 `TrafficReport`；二进制、配置和它的目录、数据目录都删了（二进制所在目录保留）；nftables 表删了；服务 inactive，没再拉起 |
| 服务器已删除 | 同样删了文件，服务 inactive |

- 卸载时日志报「卸载时有文件没删掉 … systemctl stop …: signal: terminated」，接着走了「不在 systemd 服务里运行，直接退出」：`systemctl stop --no-block` 在本服务的 cgroup 里，停止任务一开始就连它一起发了 SIGTERM。停止本身是成功的，只是判断错了
- 结束后测试锁已删、没有残留的单元、nftables 规则集为空

## 结论

- 升级、回滚、卸载的流程都符合 protocol.md「升级」「删除服务器」
- `systemctl stop --no-block` 被信号结束时要当作成功（停止任务已经交给 systemd）；已改，复测见 R20260927-agent-05

## 下一步

复测卸载（R20260927-agent-05）。

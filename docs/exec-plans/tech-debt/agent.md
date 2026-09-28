# agent 分支的技术债

规则见 [README.md](README.md)。

| 编号 | 问题 | 位置 | 优先级 | 记录日期 | 状态 |
| --- | --- | --- | --- | --- | --- |
| agent-1 | 原型的测试脚本（`v4.py` 等）只在 VPS 的 run 目录，没进仓库；run 目录超过 30 天可以被清理，要在那之前取回，提交到 `prototypes/agent-core/scripts/` | VPS `/opt/open-proxy/runs/R20260918-agent-0*/` | 中 | 2026-09-23 | 已关闭：VPS 于 2026-09-27 重装，run 目录已清空，脚本无法取回；做法留在调试记录里，集成测试的脚本重新写，直接提交进仓库 |
| agent-2 | 原型没做完的验证：V5–V7，以及 V3、V4 没覆盖的几项。等正式代码写完后统一测试 | main 的 `docs/design-docs/architecture.md`「未验证的假设」 | 高 | 2026-09-23 | 未处理（Mieru 的三项、分发出站取入站 tag、删除用户断存量连接已在 R20260927-agent-01 验证，nftables 那一项在 R20260927-agent-03 验证，待 main 更新该表；其余等集成测试） |
| agent-3 | 端口不变的重建只能先删旧入站再建（sing-box 替换同 tag 入站时先启动新的，端口被旧的占着）：新入站起不来（包括证书等配置不合法）时节点停止，和 protocol.md「应用规则」的「失败项保持原样」不一致；该文档要求的「应用前校验」也没做，证书有问题时报成各 TLS 节点的失败，从不报 `ITEM_CERTIFICATE`。改代码（入站开 `reuse_addr` 让新旧并存，或失败时按旧配置建回去）还是改文档，待定 | `agent/internal/state/apply.go` 的 `applyNode`、`applyLanding`；`agent/internal/core/core.go` 的 `SetInbound` | 高 | 2026-09-27 | 已解决（PR #7，`558ff0a`） |
| agent-4 | 失败项「保持原样」只在 Agent 不重启时成立：本地只保存最新的期望状态，没保存实际在运行的配置。Agent 重启后，之前校验不过的项按新配置建，照样失败，原来在运行的也起不来了。例如主控下发了坏证书，重启后所有 TLS 节点都会停 | `agent/internal/state/`（`Store`、`Manager.current`）；main 的 `architecture.md`「Agent 的文件」 | 高 | 2026-09-27 | 未处理 |
| agent-5 | 所有协议的节点都能经直连出站访问节点服务器本机的回环地址和内网地址（sing-box 默认不拦，mita 默认拦）：用户能连到服务器上只监听 127.0.0.1 的服务。调试记录 R20260927-agent-01 里经 Mieru 节点连到了 127.0.0.1:21999。要在分发出站里拦，域名解析到内网的情况也要考虑；测试时要能放开 | `agent/internal/core/dispatch.go` 的 `routes.pick` | 高 | 2026-09-27 | 未处理 |
| agent-6 | 升级后的新版本如果在读升级标记之前就崩溃（例如初始化时 panic），它没法自己回滚，会被 systemd 反复拉起、一直起不来。现在靠替换前试跑 `-version` 挡住大部分情况；彻底的办法是 systemd 服务里配 `StartLimitBurst` 加 `OnFailure=` 调一个回滚单元，或者安装脚本放一个换回 `.bak` 的小脚本 | `agent/internal/upgrade/`；安装脚本和 systemd 服务文件（主控生成） | 中 | 2026-09-27 | 未处理 |

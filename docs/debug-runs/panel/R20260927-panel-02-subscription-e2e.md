# R20260927-panel-02 主控 P4：订阅的端到端测试

> 摘要：在 R20260927-panel-01 的端到端测试上加订阅：7 种格式按 UA 识别和按能力表过滤节点、响应头、无效链接，再用 mihomo 直接跑生成的 Mihomo 订阅、逐个节点下载。M4 的 Agent 33 项全过（Mieru 还没实现，跳过）；换成 M6 的 Agent 后 34 项全过，Mieru 经订阅也能用。

- 状态：通过
- 日期：2026-09-27；执行：Windows 开发机 / Claude Code 会话
- 关联：[ExecPlan](../../exec-plans/active/2026-09-27-master.md) P4；开发轮次 D20260927-panel-02
- 代码：panel-subscription（合并为 `19d4924`）；agent `5b0e9ec`（M4）、`bcaa9b7`（M6）
- 环境：测试 VPS，Debian 13；客户端 sing-box 1.14.1、mihomo v1.19.31（官方发布包）

## 目的

订阅接口的输出能被真实客户端使用：Mihomo 配置能通过 mihomo 校验并经每个节点连通；其他格式按 UA 识别，只含该客户端支持的协议。

## 做法

1. `master/tests/e2e_agent.py` 加 `--mihomo`：在原有步骤（五种协议节点、两个用户、sing-box 客户端下载、停用、超额）之后测订阅
2. 取 alice 的订阅：Mihomo（UA `clash-verge … mihomo`）检查节点数、提示节点、响应头；Stash、Shadowrocket、Surge、Loon、Quantumult X 各用对应 UA 取，检查必须有和不能有的协议；`format=v2ray` 取分享链接；再取一个无效链接
3. 把 Mihomo 订阅的端口改成测试端口，加一条规则让本机下载服务走「手动选择」，`mihomo -t` 校验后运行，用 external-controller 逐个切换节点下载 20 MiB
4. e2e-3 用 M4 的 Agent，e2e-4 用 M6 的 Agent（`bcaa9b7`）。日志在 VPS 的 `/opt/open-proxy/runs/R20260927-panel-01/e2e-3`、`e2e-4`（各格式的订阅原文也在里面）

## 结果

| 运行 | Agent | 结果 | 说明 |
| --- | --- | --- | --- |
| e2e-3 | M4 `5b0e9ec` | 33 项通过 | Mieru 节点应用失败（M5 前的预期），mihomo 跳过这个节点 |
| e2e-4 | M6 `bcaa9b7` | 34 项通过 | 没有失败项，mihomo 经 SS 2022、VLESS REALITY、Hysteria2、AnyTLS、Mieru 都下载成功 |

订阅检查项（两次都通过）：

| 检查 | 结果 |
| --- | --- |
| Mihomo | YAML，五个节点加两个提示节点（剩余流量、到期）；额度不限时不发 `subscription-userinfo`；有 `profile-title`、更新间隔 24 |
| Stash / Shadowrocket / Surge / Loon / Quantumult X | 按 UA 识别；不支持的协议不出现（例如 Surge 没有 VLESS，Loon 没有 Mieru，QX 没有 Hysteria2） |
| 分享链接 | base64，五种协议加提示节点共 7 条 |
| 无效链接 | 返回提示节点「订阅链接无效」 |
| mihomo | `mihomo -t` 校验通过 |

## 结论

- 订阅的 Mihomo 输出能被 mihomo 直接使用，五种协议的参数（包括 REALITY 公钥和 short id、Hysteria2 混淆、Mieru）都对得上服务端
- 其他格式只在文本层面检查过，没有在真实的 Stash、Shadowrocket、Surge、Loon、Quantumult X 客户端上跑（都是 iOS / macOS 应用），留给集成测试时人工确认

## 下一步

P5–P7（证书、Agent 托管、备份）。

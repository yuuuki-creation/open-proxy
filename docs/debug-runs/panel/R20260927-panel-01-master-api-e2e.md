# R20260927-panel-01 主控 P1–P3：接口冒烟测试和主控 + Agent 端到端测试

> 摘要：在测试 VPS 上验证主控的骨架、管理接口、Agent 网关和期望状态。接口冒烟测试 24 项、端到端测试 18 项全部通过：Agent 上线并应用状态，SS 2022、VLESS REALITY、Hysteria2、AnyTLS 都能用，流量按节点入账，停用即时断流、恢复可用，超额停用、清零恢复。第一次端到端测试的一项失败是测试脚本没等 Agent 应用新状态，已改。

- 状态：通过
- 日期：2026-09-27；执行：Windows 开发机 / Claude Code 会话
- 关联：[ExecPlan](../../exec-plans/active/2026-09-27-master.md) P1–P3；开发轮次 D20260927-panel-01
- 代码：panel-api `faff6d3`（主控）；agent `5b0e9ec`（op-agent，M1–M4）
- 环境：测试 VPS，Debian 13，Rust 1.96.0，Go 1.26.5；客户端 sing-box 1.14.1（官方发布包）

## 目的

确认主控的管理接口行为符合 api.md，主控和 Agent 之间的认证、期望状态下发、流量上报、停用规则在真实进程上跑得通。

## 做法

1. `cargo fmt --check`、`cargo clippy --locked --all-targets -- -D warnings`、`cargo build --locked --release`（都在 `open-proxy.slice` 里）
2. `master/tests/api_smoke.py`：空数据目录启动主控（`--no-https --http-listen 127.0.0.1:28080 --public-url http://127.0.0.1:28080`），从首次初始化开始把管理接口走一遍，检查返回值和错误码
3. `master/tests/e2e_agent.py`：同一台机器上跑主控、op-agent（`MASTER_URL=http://127.0.0.1:28080`）、两个 sing-box 客户端；建一台服务器、五种协议的节点、一个套餐、两个用户；客户端经各节点从本机下载 20 MiB；再测停用、恢复、超额
4. 完整日志在 VPS 的 `/opt/open-proxy/runs/R20260927-panel-01/`

## 结果

| 测试 | 结果 |
| --- | --- |
| 接口冒烟测试 | 24 项通过：初始化只能一次、登录限流、表单格式的写操作回 415、名字和端口冲突回 409、IPv6 地址回 400、有引用时不能删、改密码后会话失效等 |
| Agent 上线 | 认证通过，版本一致（都是 dev），推送状态后 `applied_version == state_version` |
| 应用失败项 | 只有 Mieru 节点：「节点协议 Mieru 还没实现（计划 M5）」，和预期一致 |
| 各协议连通 | SS 2022、VLESS REALITY（伪装 www.microsoft.com）、Hysteria2（salamander）、AnyTLS 各下载 20 MiB，字节数完全一致 |
| 流量入账 | alice 的下行在 4 × 20 MiB 到 1.05 倍之间；日账本按节点分开 |
| 停用 / 恢复 | 停用后 alice 经 SS、VLESS 都连不上，bob 不受影响；恢复后能用 |
| 超额 | 额度改成 50 MiB 后自动停用；清零本周期后恢复；再用超后又停用、连不上 |
| 网卡 | 网速和本月用量有数（测试流量走回环，不经过网卡，数值是 SSH 等流量） |

第一次端到端测试（e2e-1）17 项通过、1 项失败：清零后脚本只等数据库里的停用原因变了就去下载，这时新状态还没推到 Agent（3 秒合并），SS 服务端回「invalid request」。脚本改成等 Agent 应用新状态后再下载（e2e-2），全部通过。

## 结论

- P1–P3 的主链路可用。architecture.md「未验证的假设」里「能注册自定义的分发出站，拨号时能从上下文里取到入站 tag」这一项在正常连接上得到印证（所有流量都经过分发出站）；其余假设（Hysteria2 停用、存量连接断开、同 tag 替换出站、Mieru、端口跳跃）留给集成测试
- sing-box 自己的日志带终端颜色码（`[31mERROR`），写进 journald 不好读：Agent 应在 sing-box 日志配置里关掉颜色（agent 分支的小改动）

## 下一步

P4 订阅；Agent 做完 M5 后把 Mieru 加进端到端测试。

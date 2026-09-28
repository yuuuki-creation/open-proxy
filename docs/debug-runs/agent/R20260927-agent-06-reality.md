# R20260927-agent-06 M8 REALITY 目标检测与扫描的运行验证

> 摘要：在测试 VPS 上用假主控发 `CheckRealityTargets` 和 `ScanRealityTargets`。检测几个公开网站和本机的测试 TLS 服务、扫描本机回环网段（按 test-vps.md 不对外扫描）都符合预期；参数不对时回 `ErrorReply`，同时两个扫描时第二个被拒绝。发现检测的延迟里含了每次的 DNS 查询，改为先解析一次，复测见 R20260927-agent-07。

- 状态：部分通过（功能全部符合预期；延迟含 DNS 查询，已改）
- 日期：2026-09-27；执行：YUCHEN（Windows）/ Claude Code 会话
- 关联：[ExecPlan](../../exec-plans/completed/2026-09-23-agent.md) M8；开发轮次 D20260927-agent-06
- 代码：临时分支 `agent-reality-wip` `d97b50a`（M7 合并前先测；内容后来原样挑到 `agent-reality`）
- 环境：测试 VPS，Debian 13；系统根证书来自 ca-certificates 包

## 目的

1. 检测：公开网站报 TLS 1.3、H2、证书有效和延迟；自签证书的服务报证书无效；连不上、解析不了、格式不对的目标报错误，不影响其他目标
2. 扫描：按并发数和速率扫，只返回支持 TLS 1.3 和 H2、证书里有域名的地址；耗时符合速率
3. 扫描参数不对（网段太大、IPv6、按速率扫不完）时整体回 `ErrorReply`
4. 同一时间只能有一个扫描

## 做法

脚本 `agent/test/scripts/m8-reality.sh`（自己加测试锁、结束清理）。假主控加了 `-tls-listen`：在 127.0.0.1:21443 开一个只做握手的测试 TLS 服务（TLS 1.3、ALPN h2、`scan-test.example.com` 的自签证书）。扫描前在测试专用表 `inet open_proxy_test` 里加一条 output 规则，把 127.0.0.2:443 转到 127.0.0.1:21443，不占用 443；扫描的网段是 127.0.0.0/29 等回环地址。检测的公开目标：www.microsoft.com、www.apple.com:443、example.com。

## 结果

检测（整个请求 0.8 秒）：

| 目标 | TLS 1.3 | H2 | 证书有效 | 延迟（毫秒） | 错误 |
| --- | --- | --- | --- | --- | --- |
| www.microsoft.com | 是 | 是 | 是 | 228 | |
| www.apple.com:443 | 是 | 是 | 是 | 266 | |
| example.com | 是 | 是 | 是 | 20 | |
| 127.0.0.1:21443（自签） | 是 | 是 | 否 | 1 | |
| 127.0.0.1:21998 | | | | | connection refused |
| no-such-host.invalid | | | | | no such host |
| bad:port:x | | | | | 目标格式不对 |

扫描：

| 请求 | 结果 |
| --- | --- |
| 127.0.0.0/29，并发 2、每秒 4 个 | 2 秒，候选只有 127.0.0.2（`scan-test.example.com`，签发者同名，1 毫秒） |
| 10.0.0.0/8 | ErrorReply：有 16777216 个地址，一次最多扫 4096 个 |
| ::1/128 | ErrorReply：只支持扫描 IPv4 网段 |
| 127.0.0.0/20，每秒 1 个 | ErrorReply：每秒 1 个、同时 4 个，扫 4096 个地址最长要 1h25m20s，超过了 18m0s |
| 127.0.0.0/26 进行中再发一个 | 第二个 ErrorReply：已经有一个扫描在进行；第一个 16 秒后返回同一个候选 |

- 结束后测试锁已删、没有残留的单元、nftables 规则集为空

## 结论

- 检测和扫描的功能符合 nodes.md「REALITY 伪装目标」、protocol.md「REALITY 目标检测与扫描」
- microsoft、apple 的延迟偏高：每次握手都重新解析域名，DNS 查询算进了延迟。改为先解析一次（有 IPv4 地址时优先），延迟只算 TCP 连接加 TLS 握手；复测见 R20260927-agent-07
- 真实网段的扫描没做：test-vps.md 要求事先得到管理员确认，留给集成测试

## 下一步

复测（R20260927-agent-07）。

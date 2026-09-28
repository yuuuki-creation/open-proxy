# R20260918-agent-01 V1 四种入站连通 + V2 的 TCP 计量

> 摘要：V1 通过——内嵌 sing-box 的四种入站都能连通，下载内容哈希一致。V2 的 TCP 部分也通过：四种协议在 100 MB 下载下计量误差都是 +0.325%（TLS 与 HTTP 开销）。意外发现：这套配置下 splice 零拷贝根本没发生，和追踪层无关，所以不存在「被 splice 绕过计数」的风险。

- 状态：通过（V2 的 UDP 和上行方向留到下一轮）
- 日期：2026-09-18；执行：Windows 开发机 / Claude Code 会话
- 关联：[原型计划](../../exec-plans/completed/2026-09-17-agent-core-prototype.md)，验证项 V1、V2（部分）；开发轮次 D20260918-agent-01
- 代码：agent `776842b`，后半程 `6faff3f`（加了 `-tracker` 开关）
- 环境：测试 VPS，Debian 12 x86_64；sing-box 库 v1.14.1（构建标签 `with_quic,with_utls`）；客户端 sing-box 1.14.1 命令行；Go 1.26.5

## 目的

1. V1：用 Go 代码内嵌启动 sing-box，四种入站是否都能连通
2. V2（TCP 部分）：按「入站 × 用户」的计量是否准确，特别是 VLESS Vision 走 splice 时会不会漏计

## 做法

- 原型 `opcore` 启动四个入站（VLESS+REALITY+Vision 20001、Hysteria2 20002、AnyTLS 20003、SS 2022 20004），自带确定性的 HTTP / HTTPS 流量源和读统计的控制接口
- 客户端用 sing-box 命令行，每个协议一个本地 SOCKS 口；`curl --socks5-hostname` 下载后比对哈希
- 计量：下载前后各读一次统计，算增量与实际字节数比较
- splice：`strace -f -e trace=splice -c` 挂在服务端进程上，跑 100 MB HTTPS 下载；再用 `-tracker=false` 关掉追踪层重测一次对比
- 所有进程用 `systemd-run --slice=open-proxy.slice` 启动；测试期间持锁

## 结果

V1，10 MB 下载，哈希与服务端计算值比对：

| 协议 | 结果 |
| --- | --- |
| VLESS + REALITY + Vision | 一致 |
| Hysteria2 | 一致 |
| AnyTLS | 一致 |
| Shadowsocks 2022 | 一致 |

V2（TCP），100 MB HTTPS 下载，实际应用层数据 104857600 字节：

| 协议 | 下行统计 | 误差 | 上行统计 |
| --- | --- | --- | --- |
| VLESS | 105197976 | +0.325% | 1085 |
| Hysteria2 | 105197931 | +0.325% | 1061 |
| AnyTLS | 105197952 | +0.325% | 1061 |
| Shadowsocks 2022 | 105197953 | +0.325% | 1061 |

splice：挂追踪层和不挂追踪层两次 100 MB HTTPS 传输，`strace` 统计里**都没有任何 splice 调用**。

资源：测试期间 slice 峰值约 1057 MB，整机内存用量 2399 MB / 11959 MB，机器上其他服务未受影响。

## 结论

- V1 通过。用 Go 代码构造配置、内嵌启动 sing-box 的路线可行，不需要 fork
- V2 的 TCP 部分通过，误差远小于 1%。多出的 0.325% 是 TLS 记录层和 HTTP 头，属于真实流量
- **splice 不是问题**：REALITY 的外层 TLS 由服务端终结，服务端拿到的不是裸 TCP 连接，splice 的前提不成立，因此 Xray Vision 那套「零拷贝绕过统计」的担心在 sing-box 这条路上不存在。计量用 sing 自带的计数连接即可
- 追踪层对 splice 没有影响（关掉也一样没有），所以将来加限速包装时，也不会因此丢掉已有的零拷贝

## 下一步

补完 V2 的上行方向和 UDP（需要给原型加一个 UDP 回显服务），然后做 V3（停用用户断连接）。

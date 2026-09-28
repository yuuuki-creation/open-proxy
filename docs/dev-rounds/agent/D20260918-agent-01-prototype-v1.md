# D20260918-agent-01 原型 agent-core：V1 与 V2 的 TCP 部分

> 摘要：写出内嵌 sing-box 的原型 `prototypes/agent-core`，在 VPS 上编译并跑通 V1（四种入站连通）和 V2 的 TCP 计量（误差 +0.325%）。顺带确认 splice 零拷贝在这条路上根本不发生，统计不会被绕过。

- 状态：完成
- 关联：[原型计划](../../exec-plans/completed/2026-09-17-agent-core-prototype.md) V1、V2（部分）；调试记录：[R20260918-agent-01](../../debug-runs/agent/R20260918-agent-01-v1-connectivity.md)
- 提交：`aa1da52`..`6faff3f`

## 目的

验证「把 sing-box 当 Go 库内嵌、用 ConnectionTracker 按用户统计」这条路走得通，这是整个 Agent 设计里风险最大的假设。

## 做了什么

- `prototypes/agent-core/`：
  - `server.go` 用 Go 代码直接拼 sing-box 配置并内嵌启动四种入站，不写配置文件
  - `tracker.go` 实现 `adapter.ConnectionTracker`，用 sing 自带的计数连接按「入站 × 用户」累计；累计值不清零，留给主控算增量
  - `creds.go` 生成 UUID、密码、SS 2022 密钥、REALITY 密钥对和自签证书
  - `testsrv.go` 提供确定性的 HTTP / HTTPS 流量源；`clientcfg.go` 生成各协议的客户端配置
  - `main.go` 串起来，并提供读统计的控制接口和 `-tracker` 开关
- 在 VPS 上编译（构建标签 `with_quic,with_utls`）并完成 V1、V2（TCP）验证，过程见调试记录

## 结果

- V1 通过：四种协议客户端都能连通，10 MB 下载哈希一致
- V2（TCP）通过：100 MB 下载，四种协议计量误差都是 +0.325%，即 TLS 与 HTTP 开销
- 计划里两个「待核实」有了答案：构建标签是 `with_utls`；splice 不发生，统计没有被绕过的风险

## 遗留问题

- V2 还差上行方向和 UDP：需要给原型加一个 UDP 回显服务
- V3–V7 未开始；Mieru 尚未接入原型
- 本机只做交叉编译检查，真实编译和验证都在 VPS 上（本轮实际做法）

## 下一轮

补完 V2（上行、UDP），然后做 V3：停用用户时断开他的存量连接。

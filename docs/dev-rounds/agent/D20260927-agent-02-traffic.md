# D20260927-agent-02 正式 Agent：M4 流量上报

> 摘要：Agent 每 10 秒把追踪层按「节点 × 用户」的累计流量、默认路由网卡的累计收发和 `boot_id` 发给主控（`TrafficReport`）；另外允许主控地址是本机回环地址时用 `http://`，方便在测试 VPS 上把主控和 Agent 装在同一台机器上测试。经 PR #8 合并，CI 全部通过。

- 状态：完成
- 关联：[ExecPlan](../../exec-plans/completed/2026-09-23-agent.md) M4；调试记录：无
- 提交：`35b331a`（工作分支 `agent-traffic`）squash 合并为 `8e073d6`（PR #8）

## 目的

按 protocol.md「上报」实现 `TrafficReport`：主控据此算每个用户在每个节点上的用量、判断超额，并用网卡计数算实时网速和整机流量。

## 做了什么

- `agent/internal/sysinfo/sysinfo.go`：从 `/proc/net/route` 找 IPv4 默认路由所在的网卡（多条时取 metric 最小的），从 `/proc/net/dev` 读它的累计收发字节，读 `/proc/sys/kernel/random/boot_id`
- `agent/cmd/op-agent/traffic.go`：每 10 秒从追踪层取累计值，按节点 ID 和用户 ID 发 `TrafficReport`，只发非零的行；没连上时跳过这一次（计数是累计值，下一次上报自然带上）；网卡读不到时这次不带网卡计数，出错原因变了才打日志；留了立即补发一次的入口，给升级、卸载前用（M7）
- `agent/internal/core/options.go`：`ParseNodeTag`、`ParseUserName`，和生成 tag、用户名的函数放在一起
- `agent/cmd/op-agent/config.go`、`agent/internal/conn/client.go`：主控地址只有本机回环地址（`localhost`、`127.0.0.0/8`、`::1`）可以用 `http://`（对应 `ws://`）；其他情况仍然只接受 `https://`，因为 Token 在第一条消息里明文发送

## 结果

- PR #8 的检查全部通过（gofmt、go vet、staticcheck、linux amd64 / arm64 编译），已合并
- 没有运行测试：计量准确性原型已测过（V1、V2），正式代码等全部写完后在测试 VPS 上统一测

## 遗留问题

- 无新增。技术债 agent-4（重启后失败项不再保持原样）仍待用户定办法

## 下一轮

M5：Mieru 服务端，连接交给 sing-box 路由。

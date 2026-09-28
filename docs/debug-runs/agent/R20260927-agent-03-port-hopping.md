# R20260927-agent-03 M6 端口跳跃规则的运行验证

> 摘要：在测试 VPS 上用假主控下发几个开了端口跳跃的 Hysteria2 节点，用 `nft` 读回 Agent 建的规则。规则内容正确；每次应用整体重建，手工加的规则被清掉、不会重复；冲突的节点单独报 `ITEM_PORT_HOPPING`；重启后按本地状态重建，没有本地状态时启动就清表；没碰别的表。没有做真实的 UDP 转发测试（要从外网发包）。

- 状态：通过
- 日期：2026-09-27；执行：YUCHEN（Windows）/ Claude Code 会话
- 关联：[ExecPlan](../../exec-plans/completed/2026-09-23-agent.md) M6；main 的 architecture.md「未验证的假设」里「nftables 端口跳跃规则能幂等地建立和清理」；开发轮次 D20260927-agent-04
- 代码：`agent-firewall` `c4f2599`
- 环境：测试 VPS，Debian 13，内核 6.12；google/nftables v0.2.1-0.20240414091927-5e242ec57806；nftables 1.1.3-1（只用来读规则）

## 目的

1. 规则的内容对：只转 IPv4、发往本机的 UDP，范围转到节点实际的端口，带节点注释
2. 每次应用整体重建：别人手工加的规则会被清掉，同样的状态推两次不会重复
3. 范围不合法、和别的节点重叠、盖住别的节点的端口时，只有这个节点报 `ITEM_PORT_HOPPING`
4. 删节点、换范围后规则跟着变；没有节点开端口跳跃时删表
5. Agent 重启后按本地状态重建；没有本地状态时启动就清掉以前留下的表
6. 不碰别的表

## 做法

脚本 `agent/test/scripts/m6-firewall.sh`（自己加测试锁、结束清理）。按 test-vps.md 的安全规则，Agent 用 `-nft-table open_proxy_test`（新加的命令行参数，默认 `op_agent`）；节点端口 21002–21006，跳跃范围在 30000–30999。VPS 上原来没有 `nft` 命令，脚本装了 nftables 包（apt，1.1.3-1），只用它读规则；装完它的服务是 disabled / inactive，没有加载任何规则。

步骤：状态 A（节点 1、2 正常；3 范围倒了；4 和 2 重叠；5 盖住别的节点的端口）→ 手工加一条规则后再推一次 A → 状态 B（节点 1 换范围，其他删掉）→ 手工加规则后重启 Agent → 状态 C（没有节点）→ 删掉本地状态、手工建表，让 Agent 在连不上主控的情况下启动。每一步 `nft list table` 读回。

## 结果

状态 A 读回的规则（节点 1、2）：

```
meta nfproto ipv4 fib daddr type local udp dport 30000-30099 redirect to :21002 comment "node-1"
meta nfproto ipv4 fib daddr type local udp dport 30100-30199 redirect to :21003 comment "node-2"
```

| 检查 | 结果 |
| --- | --- |
| 状态 A 的失败项 | 节点 3「范围 30300-30250 不合法」、节点 4「和节点 2 的 30100-30199 重叠」、节点 5「盖住了节点 1 的端口 21002」，都是 `ITEM_PORT_HOPPING`；节点 1、2 的规则照常建好 |
| 手工加规则后再推 A | 手工的规则没了，还是那两条，没有重复 |
| 状态 B | 只剩节点 1，范围变成 30300-30399 |
| 手工加规则后重启 | 按本地的状态 B 重建，手工的规则没了 |
| 状态 C | 表被删掉 |
| 没有本地状态时启动 | 手工建的表被删掉（Agent 连不上主控也照样清理） |
| 别的表 | 测试前后 `nft list tables` 都是空的；结束后 `nft list ruleset` 为空 |

## 结论

- main 的 architecture.md「未验证的假设」里「nftables 端口跳跃规则能幂等地建立和清理」成立，需要 main 更新该表
- 真实的 UDP 转发（外网发到跳跃范围，落到 Hysteria2 端口）没测：从本机发的包不经过 prerouting，要从外网发包，按 test-vps.md 要先确认才能开放端口。留到集成测试（技术债 agent-2）
- 测试 VPS 上装了 nftables 包（服务未启用），以后读规则可以直接用 `nft`

## 下一步

开 PR 合并 M6。

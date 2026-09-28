# 数据库表结构

- 状态：已采纳（2026-09-23）
- 创建：2026-09-23
- 最后更新：2026-09-27

> **摘要**：SQLite，sqlx 带版本号迁移，表之间用数字 ID 关联。配置类表：管理员、会话、设置、服务器、证书、节点、落地出口、套餐、用户；流量类表：计数器、用户日账本、服务器日账本。用户流量只存「全部累计 + 本周期起点」，按天的数据只有日账本一套。停用状态由规则算出来，不做「应该停用 / 已生效」两个字段的状态机。每台服务器的期望状态从数据库现算，内容哈希变了才升版本。正式代码写出来以后，字段以 panel 分支的迁移脚本为准。

参考了妙妙屋 X 的数据模型笔记，只借思路。流量、超额与到期的规则见 [architecture.md](architecture.md)，期望状态的内容见 [protocol.md](protocol.md)。

## 约定

- SQLite，WAL 模式，开启外键；迁移脚本放在 panel 分支的 `master/migrations/`，按编号执行
- 主键都是自增整数；表之间只用 ID 关联，名字可以随便改
- 时间：Unix 毫秒整数（UTC）。日期（开通日、到期日、日账本的「哪一天」）：`YYYY-MM-DD` 文本，按管理员设置的时区
- 流量和额度：整数字节
- 秘密分两类：
  - 只需要比对的，存 SHA-256：会话 Token、Agent Token
  - 需要再次显示或下发的，存明文：订阅 Token、用户凭据、出口密码、Cloudflare Token、证书私钥。所以数据库备份文件等同于全部凭据，面板下载备份时要提示妥善保管
- 写流量用短事务，定期做 WAL checkpoint

## 表一览

| 表 | 一行是什么 |
| --- | --- |
| `admin` | 唯一的管理员 |
| `sessions` | 一个登录会话 |
| `settings` | 一项全局设置（键值） |
| `servers` | 一台装了 Agent 的服务器 |
| `deleted_servers` | 一台已删除服务器的 Token 哈希，用来通知当时不在线的 Agent 卸载 |
| `certificates` | 一张证书：某台服务器的，或主控自己的 |
| `nodes` | 一个节点（服务器上的一个入站） |
| `exits` | 一个落地出口 |
| `plans` | 一个套餐 |
| `plan_nodes` | 套餐包含的节点 |
| `users` | 一个用户（拼车的朋友） |
| `traffic_counters` | Agent 上报的某个计数上一次的原始值 |
| `traffic_daily` | 用户日账本：某天、某用户、在某节点上的流量 |
| `server_traffic_daily` | 服务器日账本：某天、某台服务器网卡的收发 |

## 配置类表

### admin

只有一行。`username`、`password_hash`（Argon2id）。修改密码时删除 `sessions` 里的全部会话。

### sessions

`token_hash`（主键）、`created_at`、`expires_at`、`last_seen_at`、`ip`、`user_agent`。浏览器 Cookie 里是 Token 明文，库里只有哈希。

### settings

`key`（主键）、`value`（JSON）、`updated_at`。用到的键：

| 键 | 内容 |
| --- | --- |
| `domain` | 主控域名 |
| `cloudflare_api_token` | 申请证书用 |
| `timezone` | 管理员时区，例如 `Asia/Shanghai`；日账本分日、每月重置、到期都按它 |
| `acme_account` | ACME 账户密钥 |
| `template` | 订阅模板：来源（内置 / 粘贴或上传 / 远程地址）、正文、远程地址、拉取周期、上次拉取时间和错误 |

### servers

| 字段 | 说明 |
| --- | --- |
| `name` | 唯一 |
| `address` | 管理员填写的 IPv4，或解析到 IPv4 的域名；节点默认用它 |
| `port_range_start`、`port_range_end` | 自动分配节点端口的范围，默认 10000–60000 |
| `cert_mode`、`cert_domain` | 证书方式：`acme`（要填域名）或 `self_signed` |
| `traffic_quota_bytes`、`traffic_reset_day` | 整机月额度和重置日（对应 VPS 账单日），都可以不填；只用来显示 |
| `token_hash` | Agent Token 的 SHA-256，唯一 |
| `state_version`、`state_hash` | 当前期望状态的版本和内容哈希，见「期望状态怎么生成」 |
| `applied_version`、`apply_failures` | Agent 最近一次 `StateReport` 的内容；失败项是 JSON |
| `agent_version`、`agent_arch`、`agent_instance_id`、`rolled_back_from` | Agent 最近一次 `Hello` 的内容 |
| `last_seen_at` | 最近一次收到消息的时间。是否在线看内存里的连接表，不存库 |
| `nic_boot_id`、`nic_interface`、`nic_last_rx`、`nic_last_tx` | 网卡计数上一次的原始值 |

### deleted_servers

`token_hash`（主键）、`name`、`deleted_at`。见 protocol.md「删除服务器」。

### certificates

| 字段 | 说明 |
| --- | --- |
| `server_id` | 所属服务器，服务器删除时一起删；为空表示主控自己的 HTTPS 证书（只能有一张） |
| `kind` | `acme` 或 `self_signed`（自签证书由主控生成） |
| `domain` | 自签证书为空 |
| `cert_pem`、`key_pem` | 证书链和私钥 |
| `sha256` | 证书指纹；自签证书写进订阅 |
| `not_after`、`renewed_at`、`last_error` | 到期时间、上次续期时间、上次续期失败的原因 |

### nodes

| 字段 | 说明 |
| --- | --- |
| `server_id` | 所属服务器，服务器删除时一起删 |
| `name` | 订阅里显示的名字 |
| `protocol` | `vless_reality`、`hysteria2`、`anytls`、`shadowsocks2022`、`mieru` |
| `port` | 同一台服务器上唯一 |
| `hop_port_start`、`hop_port_end` | Hysteria2 端口跳跃范围，不开时为空。单独成列，分配端口时要避开 |
| `params` | JSON，各协议自己的参数：REALITY 的私钥、公钥、short ID、伪装目标；Hysteria2 的混淆密码；Shadowsocks 2022 的加密方式和服务端主密钥 |
| `address` | 单独覆盖服务器地址，为空时用服务器的 |
| `exit_id` | 落地出口，为空表示直连 |
| `enabled` | 停用的节点不下发、不进订阅 |
| `sort_order` | 在订阅里的顺序，面板上拖拽调整 |

### exits

| 字段 | 说明 |
| --- | --- |
| `name` | |
| `kind` | `third_party` 或 `self_built` |
| `host` | 第三方出口的地址；自建的为空，用落地机的地址 |
| `port`、`username`、`password` | SOCKS5 参数；自建的由主控生成 |
| `landing_server_id` | 自建时是哪台服务器当落地机，唯一；这台服务器的期望状态据此生成 SOCKS5 入站 |

### plans、plan_nodes

- `plans`：`name`（唯一）、`traffic_quota_bytes`（为空表示不限）
- `plan_nodes`：`plan_id`、`node_id`，套餐或节点删除时这一行跟着删

### users

| 字段 | 说明 |
| --- | --- |
| `name` | 唯一 |
| `remark` | 备注 |
| `plan_id` | 当前套餐 |
| `enabled` | 管理员手动停用时为假 |
| `started_on` | 开通日，决定每月哪天重置；可以改 |
| `expires_on` | 到期日，为空表示永久；到期日当天结束（管理员时区的 24 点）时停用 |
| `uuid`、`password`、`ss_key` | 凭据，所有节点通用；`uuid` 唯一 |
| `sub_token` | 订阅链接里的 Token，唯一；面板要能随时复制，所以存明文 |
| `up_total`、`down_total` | 从创建起的全部累计 |
| `period_base` | 本周期起点时的 `up_total + down_total` |
| `last_reset_at` | 最近一次自动重置的时间，保证同一周期只重置一次 |
| `blocked_reason`、`blocked_since` | 当前停用原因（空 / 手动 / 到期 / 超额）和开始时间，由检查任务按规则写，见「停用规则」 |

## 流量类表

### traffic_counters

`server_id`、`node_id`、`user_id`（联合主键）、`last_up`、`last_down`：Agent 上报的累计值上一次是多少。Agent 换了实例 ID（进程重启）时，删掉这台服务器的全部行。

### traffic_daily、server_traffic_daily

- `traffic_daily`：`day`、`user_id`、`node_id`（联合主键）、`server_id`、`up`、`down`。存 `server_id` 是为了节点删掉以后，服务器维度的历史还在
- `server_traffic_daily`：`day`、`server_id`（联合主键）、`rx`、`tx`
- 外键：`user_id` 关联用户，**删除用户时他的日账本和计数器一起删**；`node_id`、`server_id` 不设外键，节点、服务器删掉后历史流量还在，统计里显示为「已删除」

## 流量怎么算

收到一条 `TrafficReport`，在一个短事务里：

1. 实例 ID 和 `servers.agent_instance_id` 不同：删掉这台服务器的 `traffic_counters`，记下新实例 ID
2. 每一行：有上次的值就「增量 = 本次 − 上次」，没有（新实例或新出现的行）就「增量 = 本次」，因为 Agent 的计数都是从零开始的。更新 `traffic_counters`；`users.up_total`、`down_total` 加上增量；`traffic_daily` 按今天累加
3. 网卡同理，`boot_id` 变了就「增量 = 本次」；累加到 `server_traffic_daily`。第一次见到这台服务器的网卡（或换了网卡）只记为起点，不入账：那时的累计是开机以来的全部流量，不是今天的

读的时候：

- 用户本周期用量 = `up_total + down_total − period_base`
- 服务器本月用量 = 从上一个重置日起 `server_traffic_daily` 的和
- 按天、按节点、按服务器的图表：`traffic_daily` 按维度求和
- 实时网速在内存里用相邻两次网卡计数算，不存库

和 architecture.md 原来写的不同：不再「第一次见到的计数只记为起点」。Agent 的计数每次启动都从零开始，第一次见到的值就是真实流量；从备份恢复数据库时，计数器和日账本一起回到过去，增量也不会重复算。

## 停用规则

检查任务在两种时候跑：收到流量上报时，只看这次有流量的用户；每分钟一次，看全部用户（到期、每月重置）。管理员改了用户、套餐之后也立即检查一次。

- 按优先级算停用原因：手动停用 > 到期 > 超额（`plan.traffic_quota_bytes` 不为空，且本周期用量不小于它）> 不停用
- 原因变了就更新 `blocked_reason`、`blocked_since`，然后重新生成期望状态。停用和恢复走的是同一条路：用户在不在期望状态里
- 每月重置：到了重置日（开通日的「日」，短月夹到月末）在管理员时区的 0 点，令 `period_base = up_total + down_total`，写 `last_reset_at`
- 手动「清零本周期用量」：只令 `period_base = up_total + down_total`，重置日不变
- **不存「停用已生效」**：期望状态整份下发、Agent 追踪层即时断流，不需要妙妙屋 X 那种失败重试的状态机。面板在用户页显示「停用中，还有 N 台服务器没同步」：看这个用户涉及的服务器里，有几台 `applied_version` 小于 `state_version`（偏保守，但足够）。architecture.md 原来的「两个字段」写法随之改掉

## 期望状态怎么生成

- 不存期望状态本身，每次从数据库现算。一台服务器的状态包括：它上面启用的节点；能用这些节点的用户（套餐包含该节点，且没有停用）；节点用到的出口；它当落地机时的 SOCKS5 入站（允许的来源 IP = 用这个出口的节点所在服务器的地址，解析成 IP）；有 TLS 节点时它的证书
- 列表按 ID 排序后序列化、算哈希。任何配置变更、停用状态变化之后，把所有服务器的状态都重算一遍（服务器只有几台，代价很小）；哈希变了才升版本（规则见 protocol.md），再等 3 秒合并后推送
- 好处：不用在每个改动的地方判断「影响了哪些服务器」，漏判也不会出错

## 删除时的规则

| 删除 | 结果 |
| --- | --- |
| 服务器 | 节点、证书一起删；Token 哈希移到 `deleted_servers`；它是某个自建出口的落地机时，先要删掉那个出口 |
| 节点 | 从所有套餐里移除 |
| 落地出口 | 有节点在用时不能删 |
| 套餐 | 有用户绑定时不能删 |
| 用户 | 凭据和订阅链接立即失效；他的日账本和计数器一起删，历史总量随之变小 |

## 决策记录

| 日期 | 决定 | 说明 |
| --- | --- | --- |
| 2026-09-23 | 删除用户时，他的流量记录一起删 | 管理员决定；历史总量随之变小。节点、服务器删除时历史保留 |
| 2026-09-23 | 到期精确到日 | 到期日当天结束时停用，和订阅里显示的日期一致 |
| 2026-09-23 | 用户流量只存「全部累计 + 本周期起点」和日账本 | 取代 architecture.md 原来的「用户 × 节点累计表」；按节点的数据从日账本求和 |
| 2026-09-23 | 不存「停用已生效」 | 取代 architecture.md 原来的两个字段；面板按服务器同步情况显示 |
| 2026-09-23 | 期望状态现算，内容哈希变了才升版本 | 不用判断每次改动影响了哪些服务器 |
| 2026-09-23 | 第一次见到的计数按增量算 | 取代 architecture.md 原来的「只记为起点」；Agent 的计数从零开始 |

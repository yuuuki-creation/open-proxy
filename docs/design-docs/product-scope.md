# 产品定位与核心选型

- 状态：已采纳（2026-09-17 讨论完成）
- 创建：2026-09-17
- 最后更新：2026-09-23

## 背景

open-proxy 参考妙妙屋 X，目标是做一个更轻量、更简单的版本。妙妙屋 X 功能很全（12 种订阅格式、证书管理、Nginx 管理、测速、服务器共享、Telegram、MCP 等），其中很多功能在我们的场景里用不上。

## 第一版范围

| 做 | 不做 |
| --- | --- |
| 主控：Rust + React（HeroUI），单文件和 Docker 两种部署，SQLite，自动 HTTPS，单管理员；面板只做中文、只适配电脑 | 多管理员与权限、PostgreSQL、多语言、手机适配 |
| Agent：Go，内嵌 sing-box 和 Mieru，脚本安装，面板一键升级 | Agent 的 Docker 部署 |
| 用户节点协议：VLESS + REALITY、Hysteria2、AnyTLS、Shadowsocks 2022、Mieru | 其他协议 |
| 落地出口：第三方 SOCKS5、自建 SOCKS5 落地机 | 中转 |
| TLS 证书：按服务器选主控统一申请（DNS 验证，Cloudflare）或自签 | 多家 DNS 服务商 |
| 套餐（额度 + 可用节点）+ 用户（到期时间）；按开通日每月重置；上行 + 下行计量；超额或到期停用 | 限速、连接数限制 |
| 订阅：Mihomo 等 6 种客户端给完整配置（一份 Mihomo 模板翻译），v2rayN 系给分享链接；按客户端能力过滤节点；订阅里显示用量 | 用户页面、用户登录、注册、支付、工单 |
| | Telegram、测速、外部节点导入、MCP、服务器共享、Nginx 管理 |

## 决策记录

| 日期 | 决定 | 说明 |
| --- | --- | --- |
| 2026-09-17 | 场景：小圈子拼车 | 几个到几十个朋友分摊服务器；需要按人统计流量、设置到期。不做注册、支付、工单 |
| 2026-09-17 | 架构：主控 + Agent | 一个面板管理所有服务器，每台服务器装 Agent 主动连接主控 |
| 2026-09-17 | 代理内核：sing-box | 协议覆盖比 Xray 广（Hysteria2、TUIC、AnyTLS 等） |
| 2026-09-17 | 内核接入：sing-box 作为 Go 库内嵌进 Agent | 官方二进制没有按用户统计和运行时增删用户的接口（见下方调研），这些由 Agent 自己实现；Agent 因此用 Go |
| 2026-09-17 | 用户节点协议：VLESS + REALITY、Hysteria2、AnyTLS、Shadowsocks 2022、Mieru | Mieru 不在 sing-box 里，用 mieru 官方的 Go 服务端库单独内嵌进 Agent |
| 2026-09-17 | SOCKS5：只做落地出口，不作为用户节点 | 节点可以选择让流量从某个出口出去（例如住宅 IP 落地） |
| 2026-09-17 | 落地出口：第三方和自建都支持 | 第三方：在主控登记地址和账号；自建：落地机也装 Agent，开 SOCKS5 入站，只允许自己的服务器访问 |
| 2026-09-17 | 中转：第一版不做 | 朋友直连节点服务器 |
| 2026-09-17 | 客户端：Mihomo 系；iOS 的 Shadowrocket、Stash、Surge、Loon、Quantumult X；v2rayN / v2rayNG | sing-box 系客户端暂不专门支持 |
| 2026-09-17 | 计费：每人独立额度 | 每个人有自己的流量额度和到期时间 |
| 2026-09-17 | 套餐做模板 | 套餐定义流量额度和可用节点；用户绑定一个套餐，到期时间按人设置；改套餐对绑定的人全部生效 |
| 2026-09-17 | 流量按开通日每月重置 | 每个人从自己的开通日起算，每月同一天清零；开通日是 29–31 号时，没有这一天的月份在月末最后一天重置 |
| 2026-09-17 | 计量：上行 + 下行 | |
| 2026-09-17 | 超额或到期：停用，保留账号 | 加额度或续期后立即恢复，订阅链接不变；删除由管理员手动操作 |
| 2026-09-17 | 限速、连接数限制：第一版不做 | 朋友之间先靠流量额度约束；Agent 包装连接的地方留好扩展位置 |
| 2026-09-17 | 不做用户页面 | 只有管理员面板；朋友在客户端里看用量和到期时间 |
| 2026-09-17 | 订阅里显示用量 | 订阅响应带用量信息（各客户端是否显示待核实），同时在节点列表里插入「剩余 xx GB」「到期 yyyy-mm-dd」提示节点 |
| 2026-09-17 | 附加功能：第一版只做「订阅里显示用量」 | 不做 Telegram、测速、外部节点导入 |
| 2026-09-17 | 管理员：只有一个 | 没有权限系统 |
| 2026-09-17 | TLS 证书：按服务器选 | 有域名的自动申请，没有域名的用自签证书并固定指纹。申请方式后来改为主控统一用 DNS 验证申请后下发（不用 sing-box 内置 ACME），见 [nodes.md](nodes.md) |
| 2026-09-17 | ~~主控技术栈：Go + React~~ | 已被下面三条取代 |
| 2026-09-17 | 主控后端改用 Rust | 管理员决定，代码主要由 AI 编写；和 Agent 的通信协议用一份定义分别生成 Rust 和 Go 代码；前端打包进二进制 |
| 2026-09-17 | 前端：React + HeroUI | |
| 2026-09-17 | 面板只做中文、只适配电脑 | 单管理员自用；不做多语言和手机适配 |
| 2026-09-17 | 证书自动申请用 DNS 验证 | 不占用 80 端口，支持泛域名；需要 DNS 服务商的 API 凭据 |
| 2026-09-17 | 节点端口自动分配，可以手动改 | |
| 2026-09-17 | Agent 保持 Go | 讨论过 Agent 也用 Rust：Rust 没有能替代 sing-box + Mieru 的成熟实现，详见 [architecture.md](architecture.md) 的「语言选型」 |
| 2026-09-17 | 主控部署：单文件和 Docker 都提供 | 数据库只用 SQLite；两种方式都支持填域名自动申请 HTTPS 证书 |
| 2026-09-17 | Agent：脚本安装 + 面板一键升级 | 面板里复制一行命令到服务器执行；之后在面板里升级，不用 SSH |

## 订阅格式

输出哪些格式、给哪些客户端，见 [subscription.md](subscription.md) 的「输出格式」。

## sing-box 调研（2026-09-17）

来源：sing-box 官方文档的 [V2Ray API](https://sing-box.sagernet.org/configuration/experimental/v2ray-api/)、[构建标签](https://sing-box.sagernet.org/installation/build-from-source/)、[更新日志](https://sing-box.sagernet.org/changelog/)、[入站列表](https://sing-box.sagernet.org/configuration/inbound/)、[SSM API](https://sing-box.sagernet.org/configuration/service/ssm-api/)；GitHub [SagerNet/sing-box](https://github.com/SagerNet/sing-box)

- 最新稳定版 v1.14.1，1.15.0 处于 alpha
- 服务端入站支持：VLESS、VMess、Trojan、Shadowsocks、SOCKS、Hysteria2、TUIC、AnyTLS、ShadowTLS、Naive、Snell（1.14 新增）等
- **按用户统计流量**：要用 V2Ray API 的 `stats.users`，它需要 `with_v2ray_api` 构建标签，**官方发布的二进制默认不带**
- **运行时增删用户**：官方只有 SSM API 支持，而且只针对 Shadowsocks 入站；其他协议没有官方的运行时用户管理接口
- 1.14 新增的 API Service（gRPC）：更新日志里写的是状态、日志、出站组、连接追踪、网络测试等，没提到用户管理或按用户统计
- 内嵌方案的可行性（连接追踪拿到用户身份、运行时重建入站）是根据代码结构的初步判断，写正式代码前先做原型验证

## Mieru 调研（2026-09-17）

来源：GitHub [enfein/mieru](https://github.com/enfein/mieru)，`apis/server/interface.go`

- 最新 v3.37.0，许可证 GPL-3.0
- `apis/server` 是官方提供给第三方集成的服务端库：`Store(配置)` → `Start()` → 循环 `Accept()`
- `Accept()` 返回客户端连接和它发来的 socks5 请求；连接上带用户身份（实现 `UserContext`）。转发到目标地址由集成方自己做，所以按用户统计流量可以直接在 Agent 里包装连接
- 配置只能在 `Start()` 之前写入；改用户需要 `Stop()` 后新建实例。`Stop()` 不会断开已经建立的连接
- 哪些客户端支持 Mieru 待核实（Mihomo 支持）

## 许可证影响

- sing-box：GPL-3.0-or-later，附加条款：衍生作品不得使用 sing-box 的名称或暗示与其有关联
- mieru：GPL-3.0
- Agent 内嵌了两者，属于衍生作品：只要把 Agent 分发给别人（二进制或源码），Agent 就必须按 GPL-3.0 开源；只是自己部署使用则不受影响
- 主控是独立程序、通过网络和 Agent 通信，一般认为不受 Agent 的 GPL 约束
- 项目和组件命名避开「sing-box」

## 详细设计

各项详细设计见 [index.md](index.md)；还没定的见 [architecture.md](architecture.md) 的「待讨论」。

# 订阅服务

- 状态：草稿（讨论中）
- 创建：2026-09-17
- 最后更新：2026-09-27

范围见 [product-scope.md](product-scope.md)，架构见 [architecture.md](architecture.md)。参考了妙妙屋 X 源码笔记：订阅生成。

## 输出格式

| 输出 | 客户端 | 内容 |
| --- | --- | --- |
| Mihomo YAML | Clash Verge Rev、FlClash、Mihomo Party 等 | 完整配置，直接由模板生成 |
| Stash YAML | Stash | 完整配置，由 Mihomo 模板翻译 |
| Shadowrocket | Shadowrocket | 完整配置，由 Mihomo 模板翻译 |
| Surge | Surge | 完整配置，由 Mihomo 模板翻译 |
| Loon | Loon | 完整配置，由 Mihomo 模板翻译 |
| Quantumult X | Quantumult X | 完整配置，由 Mihomo 模板翻译 |
| 通用分享链接（base64） | v2rayN、v2rayNG | 只有节点 |

## 链接与格式识别

- 每个用户一个链接：`/s/<长随机 Token>`（128 位以上随机数），不需要防暴力猜测的封禁机制，保留频率限制
- 按 User-Agent 自动识别格式，也可以用参数手动指定
- 识别顺序要注意：Stash 的 UA 带 `Clash` 字样，要排在 Clash 前面；Surge Mac 排在 Surge 前面；Quantumult X 兼容多种写法

## 规则模板

- **只维护一份 Mihomo 模板**，全局生效；其他完整配置格式由系统翻译
- 默认内置一份模板；管理员可以替换
- 导入方式：在面板里粘贴或上传；或者填远程 URL，主控定时拉取更新（拉取失败保留上一份）
- 模板里用占位符标记节点插入位置，服务端把节点展开进代理组
- 不内置地区分组逻辑。默认模板只有「手动选择」和「自动选最快」两个组；需要按地区分组时，在模板里用 Mihomo 的 `filter` 自己写
- **翻译只支持明确的子集**：支持的代理组类型、规则类型、规则集格式在详细设计时列清楚。翻译不了的部分跳过，在面板上生成「兼容性报告」，列出每种格式丢掉了哪些规则
- 默认模板只使用能完整翻译到所有格式的写法
- 节点怎么进代理组：用 Mihomo 自己的写法 `include-all-proxies: true`（可配 `filter`、`exclude-filter`），服务端展开成具体的节点名，所以各格式都不用支持 filter，提示节点也不会混进代理组

### 翻译子集（2026-09-27 定）

- 代理组：`select`、`url-test`、`fallback`、`load-balance`
- 规则：`DOMAIN`、`DOMAIN-SUFFIX`、`DOMAIN-KEYWORD`、`IP-CIDR`、`IP-CIDR6`、`GEOIP`、`MATCH`；`RULE-SET` 只翻译 `behavior: classical`、`format: text` 的远程规则集（Stash 支持 `.mrs` 以外的全部；Quantumult X 的远程规则格式不同，暂不翻译）
- `dns`、`sniffer`、`tun`、`hosts`、`proxy-providers`：Mihomo 原样保留，其他格式不翻译
- 以上之外的：Mihomo 原样保留，其他格式丢掉并写进兼容性报告
- Shadowrocket 输出 Clash 风格的 YAML（节点 + 代理组 + 规则）
- 远程模板默认每 24 小时拉取一次（可设 1–720 小时），拉取失败保留上一份

## 节点与协议

- 格式转换自己写（5 种协议 × 7 种输出），不用妙妙屋 X 依赖的转换库（移植自 Sub-Store，许可证不明确）
- 显式的「客户端 × 协议」能力表，生成订阅时按表过滤；面板上提示「这个节点在某客户端里看不到」
- 凭据查不到时跳过这个节点，不回退到任何默认凭据
- 节点名去掉 `=` 和 `,`（Surge、Loon 行格式的分隔符）
- Shadowsocks 2022 加密方式固定为 `2022-blake3-aes-128-gcm`（Stash、Surge、Loon、QX 只支持 aes-128/256-gcm）
- 自签证书节点：用主控生成自签证书时记下的 SHA-256，订阅里写成固定证书指纹（各格式字段名不同，见参考笔记）

### 客户端 × 协议（妙妙屋 X 转换代码的实现，待实测）

| 协议 | Mihomo | Stash | 分享链接 | Surge | Loon | QX |
| --- | --- | --- | --- | --- | --- | --- |
| VLESS + REALITY | ✅ | ✅ | ✅ | ❌ | ✅ | ✅ |
| Hysteria2 | ✅ | ✅ | ✅ | ✅（不带混淆） | ✅ | ❌ |
| AnyTLS | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| SS 2022 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Mieru | ✅ | ❌ | ✅ | ❌ | ❌ | ❌ |

Shadowrocket：VLESS + REALITY、Hysteria2、AnyTLS、SS 2022 输出，Mieru 不输出（2026-09-27 按官方说明定，待实测）。

## 用量显示

- 所有格式都在节点列表最前面插入两个提示节点：「剩余流量 xx GB」「到期 yyyy-mm-dd」（没有到期日写「永久」）。提示节点用 aes-128-gcm 的 Shadowsocks 占位，所有客户端都能显示
- 响应头：
  - `subscription-userinfo: upload=0; download=<已用>; total=<额度>; expire=<Unix 秒>`；额度不限时不发；没有到期日时 expire 用一个很远的日期（Shadowrocket 把缺失或 0 显示成 1970）
  - `profile-title: base64:<名称>`
  - `profile-update-interval`
  - `Content-Disposition` 用 RFC 5987 写法，避免中文乱码

## 停用与无效

- 超额、到期、Token 无效时，返回 200 和一份只含提示节点的配置（例如「⚠️ 流量已用完」「⚠️ 已到期」「⚠️ 请联系管理员」），按请求的格式输出，不返回 4xx。朋友更新订阅时直接在客户端里看到原因

## 决策记录

| 日期 | 决定 | 说明 |
| --- | --- | --- |
| 2026-09-17 | Mihomo、Stash 给完整配置 | 导入即用 |
| 2026-09-17 | Surge、Loon、Quantumult X 给完整配置 | 导入即用；工作量明显更大 |
| 2026-09-17 | Shadowrocket 给完整配置 | 不再和 v2rayN 共用分享链接 |
| 2026-09-17 | 只维护一份 Mihomo 模板，系统翻译成其他格式 | 翻译支持明确的子集，丢掉的规则在兼容性报告里列出 |
| 2026-09-17 | 默认内置一份模板，管理员可以导入替换 | |
| 2026-09-17 | 模板导入：粘贴 / 上传，或远程 URL 定时拉取 | |
| 2026-09-17 | 模板全局一份 | 不按套餐区分 |
| 2026-09-17 | 不内置地区分组 | 管理员没有偏好；按地区分组交给模板里的 `filter` |
| 2026-09-17 | 采用的默认做法 | 本文「链接与格式识别」「节点与协议」「用量显示」「停用与无效」各节所列，讨论中未提出异议 |

## 待讨论 / 待核实

- 各客户端实测：协议支持、`subscription-userinfo` 和 `profile-title` 是否生效、Shadowrocket 和 v2rayN / v2rayNG 是否支持 Mieru

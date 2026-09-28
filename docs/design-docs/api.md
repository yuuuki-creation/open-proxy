# 主控 API

- 状态：已采纳（2026-09-23）
- 创建：2026-09-23
- 最后更新：2026-09-27

> **摘要**：面板用的 REST JSON API 都在 `/api/` 下，靠管理员会话 Cookie 认证；面板每 10 秒轮询一次服务器列表，拿在线状态和网速，不用 WebSocket。订阅（`/s/<Token>`）和 Agent（`/api/agent/…`）各用各的 Token。前端的 TypeScript 类型手写，和主控的请求、响应结构一一对应；正式代码写出来以后，具体字段以主控代码（panel 分支的 `master/src/api/`）为准，本文只写结构和约定。

页面见 [panel.md](panel.md)，数据见 [database.md](database.md)，Agent 通信见 [protocol.md](protocol.md)。

## 约定

- 路径：`/api/<资源复数>`，`GET` 列表和详情，`POST` 创建，`PATCH` 部分修改，`DELETE` 删除；其他动作用 `POST /api/<资源>/<id>/<动作>`
- 请求和响应都是 JSON，字段名 `snake_case`。ID 是整数；时间是 RFC 3339（UTC）；日期是 `YYYY-MM-DD`（管理员时区）；流量是整数字节，面板负责换算成 GB
- 列表不分页：用户、节点都只有几十条。流量统计按日期范围查
- 错误：HTTP 状态码加 `{"code": "...", "message": "..."}`。`code` 是英文，给前端判断；`message` 是中文，直接显示。常用状态码：400 参数不对，401 没登录，404 不存在，409 冲突（名字重复、端口被占用、还在被引用所以不能删）
- 认证：登录后发 Cookie `op_session`（HttpOnly、Secure、SameSite=Strict），有效期 30 天。写操作只接受 `application/json`，加上 SameSite 就能防跨站请求伪造
- 登录限流：同一 IP 或同一用户名连续失败 5 次，锁 15 分钟
- 实时数据：面板每 10 秒拉一次 `GET /api/servers`，和 Agent 的上报周期一致

## 不需要登录的入口

| 路径 | 用途 |
| --- | --- |
| `GET /s/{token}` | 订阅。按 User-Agent 识别格式，也可以用 `?format=` 指定；Token 无效、停用时按 subscription.md 返回提示节点 |
| `GET /api/agent/ws` | Agent 的 WebSocket，见 protocol.md |
| `GET /api/agent/install.sh` | 安装脚本。脚本里不含秘密，Token 由面板生成的安装命令作为参数传入 |
| `GET /api/agent/binary/{version}/{arch}` | Agent 二进制，安装和升级时下载；请求头 `Authorization: Bearer <Agent Token>`；只托管和主控同版本的 |
| `GET /api/setup`、`POST /api/setup` | 首次初始化：查询是否已初始化；设置管理员账号、主控域名、Cloudflare Token。谁先打开谁初始化，初始化之后这两个接口失效；部署后要尽快完成初始化，安装脚本结束时提示这一点 |
| `POST /api/auth/login` | 登录 |

## 需要登录的接口

| 资源 | 接口 | 说明 |
| --- | --- | --- |
| 会话 | `POST /api/auth/logout`、`GET /api/auth/me`、`PUT /api/auth/password` | 改密码后所有会话失效 |
| 概览 | `GET /api/overview` | 今天、本月总流量，用户用量排行，最近 30 天每日趋势 |
| 服务器 | `GET/POST /api/servers`、`GET/PATCH/DELETE /api/servers/{id}` | 列表带在线状态、Agent 版本、网速、整机用量、配置同步情况（已应用版本、失败项）。创建时返回安装命令，Token 只显示这一次。删除时 Agent 自动卸载 |
| | `POST /api/servers/{id}/install-command` | 重新生成安装命令，旧 Token 立即失效 |
| | `POST /api/servers/{id}/upgrade`、`POST /api/servers/upgrade-all` | 发出升级指令就返回；结果看列表里的 Agent 版本和「上次升级回滚了」 |
| | `POST /api/servers/{id}/reality/check` | 检测伪装目标，等 Agent 回复再返回（最多 1 分钟） |
| | `POST /api/servers/{id}/reality/scan`、`GET /api/servers/{id}/reality/scan` | 发起扫描、查询进度和结果；同一台服务器同时只跑一个，结果只放内存 |
| | `GET /api/servers/{id}/traffic?from=&to=` | 网卡每日收发 |
| 节点 | `GET/POST /api/nodes`、`GET/PATCH/DELETE /api/nodes/{id}` | 创建时不填端口就自动分配；列表带「在哪些客户端里看不到」 |
| | `PUT /api/nodes/order` | 拖拽后提交整个顺序 |
| 落地出口 | `GET/POST /api/exits`、`GET/PATCH/DELETE /api/exits/{id}` | 自建出口创建时指定落地机，账号密码由主控生成 |
| 套餐 | `GET/POST /api/plans`、`GET/PATCH/DELETE /api/plans/{id}` | 请求体里带节点 ID 列表；列表带绑定人数 |
| 用户 | `GET/POST /api/users`、`GET/PATCH/DELETE /api/users/{id}` | 续期、换套餐、手动停用或启用都是 `PATCH`；列表带本周期用量、停用原因、订阅链接 |
| | `POST /api/users/{id}/reset-period` | 清零本周期用量 |
| | `POST /api/users/{id}/reset-credentials` | 同时换订阅链接和全部凭据 |
| | `GET /api/users/{id}/traffic?from=&to=` | 按天、按节点的明细 |
| 流量统计 | `GET /api/traffic?group_by=user\|node\|server&from=&to=` | 每日流量，按维度分组 |
| 订阅模板 | `GET/PUT /api/template` | 来源：内置、粘贴或上传的正文、远程地址和拉取周期 |
| | `POST /api/template/refresh` | 立即拉取远程模板 |
| | `GET /api/template/report` | 兼容性报告：每种格式丢掉了哪些规则 |
| | `GET /api/template/preview?format=&user_id=` | 按某个用户预览某种格式的输出 |
| 设置 | `GET/PATCH /api/settings` | 域名、时区、Cloudflare Token（只写，读取时只返回是否已设置） |
| 备份 | `GET /api/backup`、`POST /api/backup/restore` | 下载时用 SQLite 在线备份，不停服务；恢复时请求体是备份文件本身（`application/octet-stream`），恢复后主控自动重启 |

## 决策记录

| 日期 | 决定 | 说明 |
| --- | --- | --- |
| 2026-09-23 | 面板轮询，不用 WebSocket | 网速本来就 10 秒刷新一次，轮询最简单 |
| 2026-09-23 | 列表不分页 | 小圈子，数据量几十条 |
| 2026-09-27 | 不生成 OpenAPI，前端类型手写 | 见 architecture.md 决策记录 |
| 2026-09-23 | 首次初始化：谁先打开谁初始化 | 管理员决定，不加初始化码；代价是部署后到初始化之前可能被人抢先，安装脚本结束时提示尽快初始化 |

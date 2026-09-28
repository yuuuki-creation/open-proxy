# D20260927-panel-02 主控 P4–P7、P9：订阅、证书、Agent 托管、备份、发布

> 摘要：写完订阅（7 种格式、模板和翻译）、证书（自签、Cloudflare DNS-01 申请和续期）、Agent 托管（安装脚本、下载、升级、卸载、REALITY 检测和扫描）、备份恢复和发布工作流。VPS 上订阅端到端 34 项、运维端到端 13 项全过，发布用的静态交叉编译验证通过。PR #13、#14、#17 已合并。ACME 没有实测。

- 状态：完成（ACME 待集成测试）
- 关联：[ExecPlan](../../exec-plans/active/2026-09-27-master.md) P4–P7、P9；调试记录：R20260927-panel-02、03、04
- 提交：`5532bbd`..`b2d5a13`（P4 `19d4924`，P5–P7 `7ed12b4`，P9 `b2d5a13`）

## 目的

按 ExecPlan 做 P4–P7 和 P9，让主控功能齐全、能打包发布；前端 P8 另由子任务在 panel-web 上做。

## 做了什么

- `src/subscription/`：按 UA 或 `format` 识别客户端；Mihomo / Stash / Shadowrocket（Clash YAML）、Surge、Loon、Quantumult X、分享链接；客户端 × 协议能力表；用量提示节点和响应头；模板（内置、粘贴、远程拉取）在服务端展开 `include-all-proxies` 和 `filter`，其他格式按翻译子集转换并出兼容性报告；预览接口
- `src/acme/`：Cloudflare DNS-01 申请、到期前 30 天续期、失败每小时最多重试一次；服务器证书随期望状态下发，主控证书热替换
- `src/agent_dist.rs`、`src/api/hosting.rs`、`assets/install.sh`：Agent 二进制托管（嵌入或 `--agent-dir`），安装脚本、带 Token 的下载、升级和全部升级、删除服务器时卸载；`scripts/sign-agent.sh` 签名工具
- `src/api/reality.rs`：REALITY 目标检测和扫描（Agent 做，主控转发、限制网段和速率）
- `src/api/backup.rs`：下载备份、上传恢复（校验后放到待恢复位置，退出码 75 重启时换库，旧库保留）
- `.github/workflows/release.yml`、`master/Dockerfile`、`master/deploy/`：发布工作流（zig + cargo-zigbuild 编 musl 静态单文件，嵌入两种架构的 Agent 和前端）、Docker 镜像、systemd 服务和部署说明
- 登录限制取 `X-Forwarded-For` 的最后一个地址（原来取第一个，可以伪造）
- 测试：`e2e_agent.py` 加订阅和 mihomo，新写 `e2e_ops.py`

## 结果

- 订阅：7 种格式都生成，mihomo 经订阅的五种协议都能用（R20260927-panel-02）
- 运维：安装、下载认证、备份恢复通过（R20260927-panel-03）
- 发布：两种架构的静态单文件都能编出，x86_64 单文件跑运维测试全过（R20260927-panel-04）

## 遗留问题

- ACME 没实测（没有域名和 Cloudflare Token）
- 除 Mihomo 外的订阅格式没在真实客户端上跑过
- 发布工作流还没在 GitHub 上跑过；Agent 签名要先在仓库里配 `AGENT_SIGNING_KEY` 和 `AGENT_SIGNING_PUBKEY`
- aarch64 单文件没实际运行过

## 下一轮

用 M7、M8 的正式 Agent 测升级、回滚、卸载、REALITY（R20260927-panel-05）；合并前端 P8。

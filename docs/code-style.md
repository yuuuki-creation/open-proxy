# 代码规范

> 摘要：标识符用英文，注释、日志、错误信息全部用中文。程序名是 `op-master`（主控）和 `op-agent`（Agent）。格式和静态检查全部交给工具：开发时在测试 VPS 上跑，PR 上由 GitHub Actions 再检查一遍，不通过不合并。生成的代码提交进仓库，CI 检查是否最新。主控不得依赖 GPL 系的库。写代码阶段先不写单元测试，代码全部写完后再统一补。

本文件属于 main 分支。原型 `prototypes/` 不受本规范约束。

## 通用

- **语言**：变量名、函数名、类型名等标识符用英文；注释、日志、错误信息全部用中文。sing-box 等库自己输出的日志保持原样
- **程序名**：主控 `op-master`，Agent `op-agent`。二进制文件、systemd 服务、安装路径、Docker 镜像都用这两个名字
- **秘密不进日志**：Token、密码、私钥、用户凭据打日志时隐去，最多留前 4 个字符
- **生成的代码提交进仓库**，CI 检查是否最新：protobuf 生成的代码（`buf generate`）。前端的 API 类型手写在 `web/src/api/types.ts`，和主控的请求、响应结构一一对应（2026-09-27 定，不生成 OpenAPI）
- **依赖**：锁文件（`Cargo.lock`、`go.sum`、`pnpm-lock.yaml`）都提交。加新依赖前先看许可证：主控和前端不得依赖 GPL、AGPL、LGPL 的库（Agent 本身就是 GPL，不受限）；主控用 cargo-deny 在 CI 里检查
- **测试**：写代码阶段先不写单元测试（2026-09-23 决定），代码全部写完后统一补单元测试和集成测试。在那之前 CI 只做编译和静态检查
- **风格**：格式化和检查全交给工具，CI 不通过就不合并，不在评审里争论格式

## 主控后端（Rust，`master/`）

- Rust stable，edition 2024，用 `rust-toolchain.toml` 固定版本；一个 crate，名字 `op-master`
- 格式化用 rustfmt 默认配置；检查用 `cargo clippy -- -D warnings`
- 错误：模块内用 `thiserror` 定义错误类型，API 层统一映射成 [api.md](design-docs/api.md) 的 `{code, message}`；`main` 和定时任务里用 `anyhow`
- 日志用 `tracing`，带结构化字段（服务器 ID、用户 ID 等），消息写中文
- 数据库用 sqlx 的运行时查询（`query` / `query_as` + `FromRow`），SQL 由集成测试覆盖（2026-09-27 定：编译期检查要在编译时连库或维护离线数据，编译只在 VPS 和 CI 上做，太绕）
- 不对远程输入（请求、Agent 消息、订阅模板）`unwrap()` 或 `expect()`；只有能证明不会失败的地方可以用，旁边注释原因
- 前端构建产物用 rust-embed 嵌进二进制

| 目录 | 放什么 |
| --- | --- |
| `master/migrations/` | sqlx 迁移脚本，按编号执行 |
| `master/src/api/` | axum 路由和处理函数，一个资源一个文件 |
| `master/src/db/` | 数据库读写，一张表或一组相关的表一个文件 |
| `master/src/agent/` | Agent 网关：WebSocket、连接表、消息收发 |
| `master/src/state/` | 期望状态的生成、哈希、推送 |
| `master/src/traffic/` | 流量上报处理、停用规则、每月重置 |
| `master/src/subscription/` | 订阅：格式识别、各格式生成、模板翻译 |
| `master/src/acme/` | 证书申请和续期 |
| `master/src/jobs/` | 定时任务 |
| `master/src/pb/` | protobuf 生成的代码 |
| `master/assets/` | 嵌进二进制的文本资源（Agent 安装脚本） |
| `master/scripts/` | 发布和测试用的脚本（Agent 签名） |
| `master/tests/` | 在测试 VPS 上跑的冒烟测试和端到端测试（Python，只用标准库） |

## 前端（TypeScript，`web/`）

- pnpm、Vite、React、TypeScript 严格模式
- 格式化和检查都用 Biome
- 路由 TanStack Router，数据请求 TanStack Query，表单 react-hook-form + zod，界面 HeroUI
- 界面文字直接写中文，不做多语言

| 目录 | 放什么 |
| --- | --- |
| `web/src/pages/` | 页面，一个页面一个目录 |
| `web/src/components/` | 多个页面共用的组件 |
| `web/src/api/` | 从 OpenAPI 生成的类型和请求封装 |
| `web/src/lib/` | 工具函数（流量单位换算、日期格式等） |

## Agent（Go，`agent/`）

- Go 版本写在 `go.mod` 里；模块路径 `github.com/yuuuki-creation/open-proxy/agent`
- 格式化用 gofmt（导入顺序用 goimports）；检查用 `go vet` 和 staticcheck
- 错误：`fmt.Errorf("做什么: %w", err)` 逐层包装；处理远程输入不许 panic
- 日志用 `log/slog`，文本格式，输出到 journald
- 构建标签 `with_quic,with_utls`；编译 linux amd64 和 arm64

| 目录 | 放什么 |
| --- | --- |
| `agent/cmd/op-agent/` | 程序入口 |
| `agent/internal/conn/` | 连主控：WebSocket、重连、消息收发 |
| `agent/internal/state/` | 应用期望状态、本地保存 |
| `agent/internal/core/` | sing-box 实例管理 |
| `agent/internal/tracker/` | 按用户统计流量、断开连接 |
| `agent/internal/mieru/` | Mieru 服务端 |
| `agent/internal/firewall/` | nftables：Hysteria2 端口跳跃 |
| `agent/internal/upgrade/` | 升级、回滚、卸载 |
| `agent/internal/reality/` | REALITY 伪装目标的检测和扫描 |
| `agent/internal/sysinfo/` | 网卡流量、`boot_id` |
| `agent/internal/pb/` | protobuf 生成的代码 |
| `agent/test/` | 测试工具（假主控 `fakemaster`、Mieru 测试客户端 `mieruclient`、五种协议的客户端 `proxyclient`）和各阶段的验证脚本 |

## CI 检查

每个分支的工作流文件属于该分支，在该分支第一个代码 PR 里建。

| 分支 | 工作流 | 检查 |
| --- | --- | --- |
| main | `protocol.yml` | `buf lint`、`buf breaking` |
| panel | `panel.yml` | 主控：rustfmt、clippy、cargo-deny、编译；前端：Biome、`tsc`、构建；生成的代码是否最新 |
| agent | `agent.yml` | gofmt、`go vet`、staticcheck、编译 linux amd64 / arm64；生成的代码是否最新 |

## 常用命令

代码初始化后以实际为准，并补进这里。

| 做什么 | 命令 |
| --- | --- |
| 生成 protobuf 代码 | 在 `master/` 或 `agent/` 下 `buf generate`（各自的 `buf.gen.yaml`） |
| 主控格式化、检查 | `cargo fmt`、`cargo clippy -- -D warnings` |
| 在测试 VPS 上编译 | `source /opt/open-proxy/env.sh` 后在 worktree 里跑，进程放进 slice（见 test-vps.md） |
| 前端格式化、检查 | `pnpm biome check --write`、`pnpm tsc --noEmit` |
| Agent 格式化、检查 | `gofmt -w .`、`go vet ./...`、`staticcheck ./...` |

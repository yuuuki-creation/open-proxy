# R20260927-panel-04 发布用的静态交叉编译

> 摘要：发布工作流用 zig + cargo-zigbuild 把主控编成 x86_64 / aarch64 的 musl 静态单文件（内嵌 Agent 和前端），这一轮在测试 VPS 上按同样的步骤编一遍。两种架构都编得出来；x86_64 单文件嵌入物正确、HTTPS 正常，运维端到端测试 13 项全过。

- 状态：通过
- 日期：2026-09-27；执行：Windows 开发电脑 / Claude Code 会话
- 关联：[master ExecPlan](../../exec-plans/active/2026-09-27-master.md) P9
- 代码：panel-hosting `be6e536`（P5–P7）；发布工作流 `.github/workflows/release.yml`（panel-release）
- 环境：测试 VPS，Debian 13；Rust 1.96.0，zig 0.16.0，cargo-zigbuild 0.23.4

## 目的

1. `cargo zigbuild --release` 能不能编出两种架构的 musl 静态单文件（ring、SQLite 两个 C 依赖要交叉编译）
2. 编出来的 x86_64 单文件能不能正常工作：嵌入的 Agent 和前端能取到，自签证书的 HTTPS 能用，运维端到端测试能过

## 做法

1. 工具：zig 0.16.0 装到 `/opt/open-proxy/toolchains/zig-0.16.0`（按 ziglang.org 的 index.json 校验 SHA-256），`cargo install --locked cargo-zigbuild@0.23.4`，`rustup target add` 两个 musl 目标
2. 嵌入物：`master/agent-dist/` 放 M6 的 Agent（amd64）并用测试私钥签名；`web/dist/` 放前端的构建产物
3. 编译：`OP_VERSION=0.0.0-test cargo zigbuild --locked --release --target <x86_64|aarch64>-unknown-linux-musl`
4. 检查：`file` 看是否静态链接；不带 `--agent-dir` 启动，取 `/api/health`、`/`、`/api/agent/install.sh`（安装脚本里的 SHA-256 应该是嵌入的 Agent 的）；再用 HTTPS（自签证书）启动取 `/api/health`
5. 用这个单文件跑 `master/tests/e2e_ops.py`

## 结果

编译（6 个并行任务，全新编译；嵌入 1 个 57.6 MB 未 strip 的 Agent 和 652 KB 的前端）：

| 目标 | 退出码 | 用时 | 大小 | `file` |
| --- | --- | --- | --- | --- |
| x86_64-unknown-linux-musl | 0 | 129 s | 75.3 MB | statically linked, stripped |
| aarch64-unknown-linux-musl | 0 | 139 s | 74.0 MB | statically linked, stripped |

x86_64 单文件的检查：

| 检查项 | 结果 |
| --- | --- |
| `/api/health` | `{"version":"0.0.0-test"}` |
| `/` 和前端路由 `/servers/1` | 都回 index.html（200） |
| 安装脚本里的 `SHA256_AMD64` | 和嵌入的 Agent 一致；没有 arm64 时为空，日志警告一次 |
| HTTPS（没有证书时的临时自签证书） | `/api/health` 正常，证书 CN=open-proxy |
| `e2e_ops.py` | 第一次在「安装脚本填好了版本」失败：测试写死了版本 dev。改成从 `/api/health` 取（`be6e536`）后 13 项全过 |

- 顺带改了取嵌入 Agent 的方式：原来复制一份到堆上，改为直接引用二进制的只读段（`be6e536`）
- Agent 用 `-s -w` 去掉符号后是 40.0 MB（用 strip 实测），正式发布嵌两种架构，主控单文件约 98 MB
- 日志：`/opt/open-proxy/runs/R20260927-panel-04/`

## 结论

- 发布工作流的交叉编译方案可行，工作流里固定 zig 0.16.0、cargo-zigbuild 0.23.4（和这一轮相同）
- 单文件约 98 MB，大头是两种架构的 Agent；可以接受，先不压缩（压缩后启动要解压到内存，反而多占内存）
- aarch64 单文件没有实际运行过（VPS 是 x86_64）

## 下一步

- 打第一个标签，在 GitHub Actions 上跑一次完整的发布流程
- 有 arm64 机器时跑一次 aarch64 单文件

# main 分支状态

> 新会话先读 `AGENTS.md` 和当前分支的 `docs/status/<分支>.md`，通常就够开始工作。本文件不超过 40 行，每轮开发结束时**整篇重写**，不要在末尾追加。

- 更新：2026-09-27，[D20260927-main-01](../dev-rounds/main/D20260927-main-01-vps-reinit.md)
- main 分支只放文档和规范（含协议定义），没有代码
- 代码在 panel（主控 + 前端）和 agent（Agent + 原型）两个分支；看它们的状态不用切换分支：
  `git show origin/agent:docs/status/agent.md`、`git show origin/panel:docs/status/panel.md`
- 设计总览：先读 `docs/design-docs/product-scope.md` 的「第一版范围」和 `architecture.md` 开头的摘要

## 当前阶段

- **先把正式代码写完，再统一测试**（2026-09-23 决定）。还没实测的假设列在 `architecture.md` 的「未验证的假设」
- 已定的设计：通信协议（`protocol.md`，定义在 `protocol/openproxy/agent/v1/`）、数据库表结构（`database.md`）、主控 API（`api.md`）；代码规范 `docs/code-style.md`。panel、agent 都可以开工了
- 编译放在 GitHub Actions 上：改代码走工作分支 + PR（见 `docs/git-workflow.md`「PR 与编译」）；改 `protocol/` 也走 PR，PR 上自动跑 buf 检查

## 下一步

1. 订阅模板翻译支持的子集（`subscription.md`「待讨论」），写订阅生成前要定
2. 发布打包（`architecture.md`「待讨论」），写发布流程前要定。Agent 的文件和配置格式已在写 Agent 计划时定下（`architecture.md`「发布、安装与升级」）
3. panel、agent 开工中发现设计问题时，回到 main 改设计文档

## 测试 VPS

- 2026-09-27 重装成 Debian 13 后重新初始化完（<VPS_IP>，root）：内存 slice 7 GiB、目录、git、Go 1.26.5（装在 `/opt/open-proxy/toolchains/`）、只读 Deploy Key、仓库克隆和两个 worktree
- 重装后管理员的其他服务都不在了，以后可能再部署；规则照旧：我们的所有进程在 `open-proxy.slice` 里跑，不碰 Docker
- 还没装 Rust、Node 和 `nft` 命令，测试阶段需要时再装
- 其他开发电脑要重新把公钥加进 VPS、更新本机 `known_hosts`（见 `docs/test-vps.md`「连接」）；各开发电脑要配 SSH 别名 `op-test`（这台 Windows 开发电脑目前配的是 `testvps`）
- 待管理员决定：GitHub 上重装前的旧 Deploy Key 删不删；SSH 仍允许 root 用密码登录

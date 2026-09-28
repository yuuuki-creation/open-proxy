# main 分支状态

> 新会话先读 `AGENTS.md` 和当前分支的 `docs/status/<分支>.md`，通常就够开始工作。本文件不超过 40 行，每轮开发结束时**整篇重写**，不要在末尾追加。

- 更新：2026-09-27，写回 Agent M5–M8 的设计结论，定下「代理用户不能访问服务器本机和内网」（main `3519e78`）；上一轮记录 [D20260927-main-01](../dev-rounds/main/D20260927-main-01-vps-reinit.md)
- main 分支只放文档和规范（含协议定义），没有代码
- 代码在 panel（主控 + 前端）和 agent（Agent + 原型）两个分支；看它们的状态不用切换分支：
  `git show origin/agent:docs/status/agent.md`、`git show origin/panel:docs/status/panel.md`
- 设计总览：先读 `docs/design-docs/product-scope.md` 的「第一版范围」和 `architecture.md` 开头的摘要

## 当前阶段

- 正式代码基本写完：Agent M1–M8 已合并；主控后端 P1–P7 和发布 P9 已合并；前端 P8 在 panel-web 上进行中
- `architecture.md`「未验证的假设」已逐项填上结果：大部分成立，Mieru 的 `Stop()` 那条不成立（已改设计）；还剩 3 项留给集成测试
- 编译和测试都在测试 VPS 上做，PR 上的 CI 是合并前的最后检查（见 `docs/git-workflow.md`「PR 与编译」）

## 下一步

1. agent 分支修 agent-5（代理用户不能访问服务器本机和内网，设计见 `architecture.md` 同名一节）。代码在 agent-guard（PR 18，草稿），还没在 VPS 上跑验证
2. panel 分支写集成测试计划（未验证的假设、ACME、各订阅客户端），再补两边的单元测试
3. 开工中发现设计问题时，回到 main 改设计文档

## 测试 VPS

- 2026-09-27 重装成 Debian 13 后重新初始化完（<VPS_IP>，root）；我们的所有进程在 `open-proxy.slice` 里跑，不碰 Docker
- 已装：Go 1.26.5、Rust 1.96.0（含两个 musl 目标）、Node v24.14.1 + pnpm 10、zig 0.16.0、cargo-zigbuild 0.23.4（`source /opt/open-proxy/env.sh`）；`nftables` 包（服务没启用）；客户端 sing-box 1.14.1、mihomo v1.19.31
- 这台 Windows 开发电脑的 SSH 别名是 `testvps`，文档里的 `op-test` 要各开发电脑自己配

## 待管理员决定或提供

- GitHub 上重装前的旧 Deploy Key 删不删；SSH 仍允许 root 用密码登录
- 测 ACME 用的域名和 Cloudflare API Token
- 发布前在仓库配 Secret `AGENT_SIGNING_KEY`、Variable `AGENT_SIGNING_PUBKEY`（见 panel 分支 `master/deploy/README.md`）

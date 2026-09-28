# panel 分支状态

> 新会话先读 `AGENTS.md` 和本文件，通常就够开始工作。本文件不超过 40 行，每轮开发结束时**整篇重写**，不要在末尾追加。

- 更新：2026-09-27，主控后端 P1–P7、P9 已合并，这次会话在此暂停（记录：[D20260927-panel-02](../dev-rounds/panel/D20260927-panel-02-master-features.md)）
- 本分支内容：主控后端 `master/`（Rust）、前端 `web/`（React + HeroUI）、发布工作流 `.github/workflows/release.yml`
- 计划：[docs/exec-plans/active/2026-09-27-master.md](../exec-plans/active/2026-09-27-master.md)（P1–P9）；技术债 `docs/exec-plans/tech-debt/panel.md`

## 当前阶段

- 后端功能写完：管理接口、Agent 网关和期望状态、流量和停用、订阅 7 种格式、证书、Agent 托管（安装、升级、卸载、REALITY）、备份恢复、发布
- 没合并的工作分支：
  - `panel-web`（`1296ab1`）：前端 P8，panel.md 的页面都写了，VPS 上 biome、tsc、build 通过，对真主控的冒烟测试走通主要流程（D20260927-panel-03、R20260927-panel-06）；还没开 PR，界面排版没人看过，拖拽排序、REALITY、升级、备份恢复、删除没实测
  - `panel-e2e-upgrade`：`e2e_ops.py` 加了升级、回滚、卸载、REALITY，安装脚本加了 `OP_AGENT_ARGS`；**还没在 VPS 上跑**（当时锁被占用），还没开 PR。调试记录 R20260927-panel-05 里有编好的文件位置和做法

## 下一步

1. 跑 R20260927-panel-05：VPS 上 `/opt/open-proxy/runs/R20260927-panel-05/` 已有两个版本的 Agent（带测试公钥）、签名和主控（`0.0.2-test`），按记录的做法加锁后运行，通过后开 PR 合并
2. agent 分支修完 agent-5 以后，Agent 默认拒绝访问本机和内网：`e2e_agent.py` 经代理从本机下载，要给 Agent 加 `-allow-private-targets`（`e2e_ops.py` 不经代理传流量，不用加）
3. 前端 P8：开 PR（会第一次跑 `panel.yml` 的 web 作业），看界面排版，补测没实测的操作，然后合并。两个 index 的新行合并时会冲突，保留两边
4. 写集成测试计划（未验证的假设、ACME、各订阅客户端），再补单元测试

## 阻塞

- ACME 实测要一个域名和 Cloudflare API Token（管理员提供）
- 发布前要在仓库配 Secret `AGENT_SIGNING_KEY`、Variable `AGENT_SIGNING_PUBKEY`（见 `master/deploy/README.md`）

## 本分支要点

- VPS 上编译：`source /opt/open-proxy/env.sh`，在 `/opt/open-proxy/src/panel` 检出要测的提交，进程都放进 `open-proxy.slice`；跑测试前按占用锁规则加锁，跑完删锁
- 测试脚本在 `master/tests/`，用法写在各脚本开头；测试用的 Agent 签名私钥在 VPS 的 `/opt/open-proxy/runs/R20260927-panel-01/test-sign-key.pem`（只用于测试）
- 记录：开发轮次 `docs/dev-rounds/panel/`，调试 `docs/debug-runs/panel/`，编号带 `panel`
- 设计文档、协议定义属于 main：需要改就到 main 上改，再 `git merge origin/main`
- 主控不得引用 sing-box、Mieru 或 agent 分支的代码，避免受 GPL 约束

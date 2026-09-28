# D20260918-main-02 填入 VPS 信息并初始化

> 摘要：把测试服务器的连接信息填进 AGENTS.md，按规范初始化 VPS。发现机器上还跑着管理员自己的服务，内存上限相应下调到 7 GiB，规范里补了对应的约束。

- 状态：完成
- 关联：无 ExecPlan；调试记录：[R20260918-main-01](../../debug-runs/main/R20260918-main-01-vps-init.md)
- 提交：`10c9d38`..本记录所在的提交

## 目的

让 VPS 可用，作为后续所有编译和验证的环境。

## 做了什么

- `AGENTS.md`：填入 IP <VPS_IP>、SSH 用户 root、系统 Debian 12 x86_64
- VPS：建内存 slice（7 GiB）、目录、只读 Deploy Key、仓库克隆和 panel / agent 两个 worktree
- `docs/test-vps.md`：新增「这台机器上还有别的服务」一节；内存上限改成实际数值；「一次性初始化」标记为已完成并改成实际步骤（系统自带 Go，暂不装 Rust 和 Node）
- 新增 `docs/debug-runs/main/` 索引和本轮的调试记录

## 结果

VPS 就绪，可以开始在上面编译和跑原型。

## 遗留问题

- 各开发电脑还没配 SSH 别名 `op-test`（本轮临时用 `root@IP` 连接）
- panel 分支要用的 Rust、Node 工具链还没装

## 下一轮

D20260918-agent-01：在 agent 分支建原型模块，开始 V1。

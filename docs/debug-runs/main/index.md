# main 分支的调试记录

规则和模板见 [../README.md](../README.md)。

| run-id | 日期 | 目的 | 状态 | 结论 |
| --- | --- | --- | --- | --- |
| [R20260927-main-01](R20260927-main-01-vps-reinit.md) | 2026-09-27 | VPS 重装后重做初始化 | 通过 | Debian 13；slice（仍是 7 GiB）、目录、git、Go 1.26.5、新的只读 Deploy Key、仓库和两个 worktree 就绪；管理员的服务重装后都不在了 |
| [R20260918-main-01](R20260918-main-01-vps-init.md) | 2026-09-18 | 初始化测试 VPS | 通过 | 内存 slice（7 GiB 上限）、目录、只读 Deploy Key、仓库克隆和两个 worktree 就绪；机器上还跑着管理员自己的服务，已按此下调内存上限 |

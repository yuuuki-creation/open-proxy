# D20260927-main-01 VPS 重装后重做初始化

> 摘要：测试 VPS 被重装成 Debian 13，之前的配置和管理员的其他服务都没了。重做了一次性初始化，VPS 已能编译和测试；AGENTS.md、test-vps.md 里过时的机器信息已更新。

- 状态：完成
- 关联：调试记录 R20260927-main-01
- 提交：`6fa5f7b`（开始时的记录）；本轮收尾的提交

## 目的

让测试 VPS 恢复到能编译和测试的状态；AGENTS.md、test-vps.md 里的系统版本、「机器上还有别的服务」等信息和实际一致。

## 做了什么

- 测试 VPS：按 test-vps.md 重做一次性初始化，过程和结果见 R20260927-main-01
- GitHub：加了新的只读 Deploy Key
- `AGENTS.md`：系统改为 Debian 13；内存保护的说明改成和 test-vps.md 一致（上限 7 GiB，原来写的「给系统留 1 GiB」是初始化前的计划）
- `docs/test-vps.md`：「管理员自己的服务」一节改为重装后的情况，规则不变；「连接」补上 VPS 重装后每台开发电脑要做的事；「一次性初始化」指向新记录，写明 git、Go 的装法和校验方法、GitHub 主机公钥的来源；编译流程写明 Go 的路径
- 这台 Windows 开发电脑：核对指纹后把本机记录的 VPS 旧主机密钥换成新的，原文件备份为 `~/.ssh/known_hosts.before-vps-reinstall-20260927`

## 结果

- VPS 可以编译和测试：slice、目录、git、Go 1.26.5、Deploy Key、仓库和两个 worktree 都已就绪

## 遗留问题

- GitHub 上还留着重装前的旧 Deploy Key（私钥已随重装丢失），删不删由管理员决定
- SSH 仍允许 root 用密码登录，按规范不改，由管理员决定
- 这台 Windows 开发电脑的 SSH 别名还是 `testvps`，不是规范里的 `op-test`

## 下一轮

回到 agent 分支做 M4 流量上报。

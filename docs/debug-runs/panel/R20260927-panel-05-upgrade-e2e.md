# R20260927-panel-05 主控 + 正式 Agent：升级、回滚、卸载、REALITY 检测和扫描

> 摘要：用主控驱动完成 M7、M8 的正式 Agent，端到端验证升级（拒绝、连不上主控时回滚、成功）、删除服务器时 Agent 自己卸载、REALITY 检测和扫描。结论见「结论」。

- 状态：进行中
- 日期：2026-09-27；执行：Windows 开发电脑 / Claude Code 会话
- 关联：[master ExecPlan](../../exec-plans/active/2026-09-27-master.md) P6、P7；Agent M7、M8
- 代码：panel-e2e-upgrade；agent `198591e`
- 环境：测试 VPS，Debian 13；Go 1.26.5，Rust 1.96.0

## 目的

1. 升级：主控托管的 Agent 没签名时不发指令；签名不对时 Agent 拒绝、二进制不变；新版本连不上主控时 3 分钟后换回旧版本，主控看到「上次升级回滚了」；正常升级后版本一致、恢复同步、保留备份
2. 删除服务器时在线的 Agent 卸载自己：服务、二进制、配置、数据目录都删掉
3. REALITY 检测公网目标能报出 TLS 1.3、证书；扫描的参数校验、同时只扫一个、扫完出结果

## 做法

1. 从 agent `198591e` 编两个版本的 Agent：`0.0.1-test`（旧）和 `0.0.2-test`（新），都注入测试公钥（`R20260927-panel-01/test-sign-key.pem` 导出）；新版本用测试私钥签名
2. 主控用 `OP_VERSION=0.0.2-test` 编译
3. 跑 `master/tests/e2e_ops.py --old-agent <旧版本> --agent-dir <新版本和签名>`，步骤写在脚本里：
   - 先托管旧版本（没有签名）安装。安装脚本加了 `OP_AGENT_ARGS`，测试时让 Agent 用测试专用的 nftables 表 `open_proxy_test`
   - 换成签名是随机数据的目录，再换成正确的目录，每次重启主控
   - 回滚：Agent 回复接受升级后立即停掉主控，新版本连不上，等它换回旧版本，再启动主控
   - REALITY 扫描只扫 `127.0.0.0/29`（本机回环，不出网卡）；检测的是 www.microsoft.com、www.apple.com 和一个不存在的域名
4. 按占用锁规则加锁，跑完删锁

## 结果

还没跑（2026-09-27 会话暂停时 VPS 的锁被前端测试占着）。已经在 VPS 上准备好，都在 `/opt/open-proxy/runs/R20260927-panel-05/`：

| 文件 | 内容 |
| --- | --- |
| `op-agent-0.0.1-test`、`op-agent-0.0.2-test` | agent `198591e` 编的两个版本，注入了测试公钥 |
| `agent-new/` | 新版本和测试私钥的签名 |
| `op-master` | 本分支 `0c7784f` 编的主控，`OP_VERSION=0.0.2-test` |

继续时：加锁，然后在 `open-proxy.slice` 里运行
`python3 master/tests/e2e_ops.py --master <run 目录>/op-master --agent-dir <run 目录>/agent-new --old-agent <run 目录>/op-agent-0.0.1-test --workdir <run 目录>/ops`，
跑完删锁。这个测试不经代理传流量，agent-5（拒绝访问本机和内网）合并后也不用加 `-allow-private-targets`；但如果要测合并后的 Agent，要按做法第 1 步重编两个版本。

## 结论

## 下一步

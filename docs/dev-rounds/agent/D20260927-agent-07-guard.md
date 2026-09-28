# D20260927-agent-07 禁止代理用户访问节点服务器本机和内网（暂停）

> 摘要：修技术债 agent-5 的代码写完、推送到工作分支 `agent-guard`（PR #18，已转为草稿，未合并）：分发出站在拨号前检查目标，直连的域名先解析、过滤后按剩下的地址拨号，UDP 每个包都查；加了测试用参数 `-allow-private-targets` 和 `-log-level`。运行验证没做（测试锁被占用），管理员要求收尾暂停。sing-box 日志的颜色码在 M5 已经关掉；agent-4 这轮不做。

- 状态：部分完成
- 关联：[ExecPlan](https://github.com/yuuuki-creation/open-proxy/blob/agent-guard/docs/exec-plans/active/2026-09-27-agent-guard.md)（在工作分支 `agent-guard` 上，合并后在 `docs/exec-plans/active/`）；调试记录 R20260927-agent-08（同在工作分支上，未执行）
- 提交：工作分支 `agent-guard`：`9ef2144`、`a268126`（从 agent `198591e` 拉出）

## 目的

按主会话定的做法修 agent-5（高）：代理用户不能经节点访问节点服务器本机和内网，所有入站、TCP 和 UDP 都管，面板里不加开关；后来补的要求：加只给测试用的命令行参数 `-allow-private-targets`。顺带确认 sing-box 日志没有颜色码，判断 agent-4 能不能这轮做。

## 做了什么

- `agent/internal/core/guard.go`（新）：地址判断（回环、私有、链路本地、未指定、组播、100.64.0.0/10、0.0.0.0/8、255.255.255.255、本机网卡地址）、本机地址缓存一分钟、直连域名的解析和过滤、UDP 逐包检查的连接包装、拒绝日志（调试级别，限流）
- `agent/internal/core/dispatch.go`：分发出站实现 `ConnectionHandler` / `PacketConnectionHandler`，拨号前检查，拒绝的不交给连接管理；`core.go` 加 `Options.AllowPrivateTargets`
- `agent/cmd/op-agent/main.go`：`-log-level`、`-allow-private-targets`（开了打警告）
- `agent/test/proxyclient`（新）：五种协议的测试客户端，兼回显服务和 REALITY 密钥生成；`agent/test/scripts/guard.sh`：默认模式和带参数两段验证
- 做法和取舍写在计划的「决策记录」

## 结果

- `9ef2144` 在测试 VPS 上编译和静态检查全部通过；`a268126`（加了参数）只推送了，PR #18 上的 CI 会检查
- 运行验证没做：开始时测试锁被 R20260927-panel-web-01 占着，之后管理员要求收尾暂停

## 遗留问题

- 剩下的步骤列在计划的「进度」里：`a268126` 的编译检查、运行验证（`guard.sh`，run-id R20260927-agent-08）、PR 合并、main 的 architecture.md「不能访问服务器本机和内网」补 `-allow-private-targets`
- agent-4 留在技术债（原因和建议做法写在技术债里）；agent-6 没动

## 下一轮

拿到测试锁后跑 `guard.sh`，结果符合就把 PR #18 转回正式、合并，收尾。

# panel 分支的技术债

规则见 [README.md](README.md)。

| 编号 | 问题 | 位置 | 优先级 | 记录日期 | 状态 |
| --- | --- | --- | --- | --- | --- |
| panel-1 | ACME（Cloudflare DNS-01 申请、续期、主控证书热替换）没有实测，缺域名和 Cloudflare API Token | `master/src/acme/` | 中 | 2026-09-27 | 未处理 |
| panel-2 | 除 Mihomo 外的订阅格式（Stash、Shadowrocket、Surge、Loon、Quantumult X）只做了文本检查，没在真实客户端上跑过 | `master/src/subscription/` | 中 | 2026-09-27 | 未处理 |
| panel-3 | 发布工作流还没在 GitHub 上跑过（要先打标签、配签名密钥）；aarch64 单文件没实际运行过 | `.github/workflows/release.yml` | 中 | 2026-09-27 | 未处理 |
| panel-4 | 主控还没有单元测试（代码规范定的是代码写完后统一补） | `master/src/` | 中 | 2026-09-27 | 未处理 |

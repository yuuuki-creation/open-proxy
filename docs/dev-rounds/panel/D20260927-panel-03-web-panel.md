# D20260927-panel-03 P8 面板前端第一版

> 摘要：在工作分支 panel-web 上从零写了 `web/`：全部 11 个页面、请求封装和手写类型、公共组件，panel.yml 加 web 作业。VPS 上 Biome、tsc、构建都通过；浏览器冒烟测试走通了主要流程，发现的 3 个问题已修。还没开 PR，拖拽排序、REALITY 检测扫描、升级、备份恢复没有实测。

- 状态：部分完成
- 关联：[主控计划](../../exec-plans/active/2026-09-27-master.md) P8；调试记录：[R20260927-panel-06](../../debug-runs/panel/R20260927-panel-06-web-smoke.md)
- 提交：868a4e1..8f167d2（分支 panel-web，中间合并过一次 panel：d6db8a1）

## 目的

按 panel.md 和主控已有接口写完面板前端，VPS 上检查通过后开 PR 到 panel。

## 做了什么

- 脚手架：`web/package.json`（pnpm 10.34.5）、`vite.config.ts`（dev / preview 代理地址用 `OP_MASTER_URL`）、`tsconfig.json`（严格模式，单个配置，`tsc --noEmit` 能检查到全部代码）、`biome.json`、`pnpm-lock.yaml`
- 依赖：React 19.3、HeroUI 3.2.6（Tailwind 4.3，`@import "@heroui/styles"`，不需要 Provider）、TanStack Router 1.170（代码里定义路由表，页面按需加载）、TanStack Query 5.104、react-hook-form 7.89 + zod 4.6、recharts 3.10、@gravity-ui/icons；TypeScript 6.0（和 Vite 官方模板一致）、Vite 8、Biome 2.5。全部是 MIT / Apache-2.0，没有 GPL 系
- `web/src/api/`：`client.ts`（写请求一律 JSON，没有请求体发 `{}`；错误转成 `{code, message}`；备份下载和恢复）、`types.ts`（和 `master/src/api/*.rs` 一一对应，含 P4–P7 的模板、升级、REALITY、备份）、`endpoints.ts`、`queries.ts`
- `web/src/queryClient.ts`：401 统一跳登录页（登录后回原页面）；改动类请求失败弹出主控的错误信息，成功后刷新全部查询
- `web/src/components/`：侧边导航布局、弹窗表单、确认框、react-hook-form 和 HeroUI 的桥接、时区选择、用量条、日期范围、每日流量堆叠柱状图（1024 进制刻度，超过 8 个对象合并成「其他」）、REALITY 检测和扫描工具
- `web/src/pages/`：初始化、登录、概览、服务器（列表 10 秒刷新、详情、安装命令只显示一次、重新生成、升级 / 全部升级、删除）、节点（四步创建向导、编辑、拖拽排序、启用开关、看不到的客户端）、落地出口、套餐、用户（续期、清零本周期、停用启用、复制订阅链接、重置凭据、删除、按天按节点明细）、流量统计、订阅模板（来源切换、上传、兼容性报告、预览）、设置（域名时区、Cloudflare Token、改密码、备份下载和恢复）
- `.github/workflows/panel.yml`：加 `web` 作业（pnpm/action-setup@v6、setup-node@v7 Node 24，install / biome ci / tsc / build）

## 结果

- 8f167d2 在 VPS 上 `pnpm biome ci .`、`pnpm tsc --noEmit`、`pnpm build` 全部通过；最大的代码块 358 KB（图表），没有体积警告
- 浏览器冒烟测试见 R20260927-panel-06：初始化、登录 / 退出 / 回跳、服务器、节点向导、出口、套餐、用户、续期、流量统计、模板预览、改密码都走通

## 遗留问题

- 还没开 PR（管理员要求收尾）；CI 的 web 作业还没在 GitHub 上跑过
- 没实测：节点拖拽排序（需要真鼠标）、REALITY 检测和扫描（需要在线 Agent）、升级 / 全部升级、备份下载和恢复、删除类操作
- 内置浏览器面板隐藏时截不了图，界面排版还没人看过
- 「检测通过才能保存」由前端强制（主控不检查）；Agent 离线时 VLESS 节点建不了

## 下一轮

主会话用浏览器看界面排版，补测上面没测的操作，然后从 panel-web 开 PR 到 panel。

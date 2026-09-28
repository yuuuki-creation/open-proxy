import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig, loadEnv } from "vite";

// 构建产物在 dist/，主控编译时用 rust-embed 嵌进二进制（master/src/web.rs），面板在域名根路径。
// 本地调试（pnpm dev / pnpm preview）时把 /api 转给主控的明文 HTTP 监听（op-master --http-listen），
// 地址用环境变量 OP_MASTER_URL 指定，默认 http://127.0.0.1:8080。
export default defineConfig(({ mode }) => {
  const env = loadEnv(mode, ".", "OP_");
  const master = env.OP_MASTER_URL || "http://127.0.0.1:8080";
  return {
    plugins: [react(), tailwindcss()],
    server: {
      proxy: { "/api": master },
    },
    preview: {
      proxy: { "/api": master },
    },
    build: {
      outDir: "dist",
      emptyOutDir: true,
    },
  };
});

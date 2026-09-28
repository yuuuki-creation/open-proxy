import { I18nProvider, Toast } from "@heroui/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { RouterProvider } from "@tanstack/react-router";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "./index.css";
import { queryClient, setUnauthorizedHandler } from "./queryClient";
import { router } from "./router";

// 会话过期（任何请求返回 401）：清掉缓存，回登录页，登录后回到原来的页面
setUnauthorizedHandler(() => {
  const { pathname, href } = router.state.location;
  if (pathname === "/login" || pathname === "/setup") {
    return;
  }
  queryClient.clear();
  void router.navigate({ to: "/login", search: { redirect: href } });
});

const root = document.getElementById("root");
if (root) {
  createRoot(root).render(
    <StrictMode>
      <I18nProvider locale="zh-CN">
        <QueryClientProvider client={queryClient}>
          <RouterProvider router={router} />
          <Toast.Provider placement="top end" />
        </QueryClientProvider>
      </I18nProvider>
    </StrictMode>,
  );
}

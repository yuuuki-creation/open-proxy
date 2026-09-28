// 路由（TanStack Router，代码里定义路由表）。面板在域名根路径，用浏览器历史路由：
// 主控对找不到的路径一律返回 index.html（master/src/web.rs）。

import type { QueryClient } from "@tanstack/react-query";
import {
  createRootRouteWithContext,
  createRoute,
  createRouter,
  lazyRouteComponent,
  Outlet,
  redirect,
} from "@tanstack/react-router";
import { isUnauthorized } from "./api/client";
import { meQuery, setupQuery } from "./api/queries";
import { AppLayout } from "./components/AppLayout";
import { LoadingBlock } from "./components/QueryView";
import { NotFoundPage, RouteErrorPage } from "./components/RouteStatus";
import { queryClient } from "./queryClient";

interface RouterContext {
  queryClient: QueryClient;
}

const rootRoute = createRootRouteWithContext<RouterContext>()({
  component: Outlet,
  notFoundComponent: NotFoundPage,
  errorComponent: RouteErrorPage,
});

/** 首次打开：还没初始化时设置管理员账号等；初始化以后这个页面跳去登录。 */
const setupRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/setup",
  beforeLoad: async ({ context }) => {
    const status = await context.queryClient.fetchQuery(setupQuery);
    if (status.initialized) {
      throw redirect({ to: "/login" });
    }
  },
  component: lazyRouteComponent(() => import("./pages/setup/SetupPage"), "SetupPage"),
});

const loginRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/login",
  validateSearch: (search: Record<string, unknown>): { redirect?: string } => ({
    redirect: typeof search.redirect === "string" ? search.redirect : undefined,
  }),
  beforeLoad: async ({ context }) => {
    const status = await context.queryClient.fetchQuery(setupQuery);
    if (!status.initialized) {
      throw redirect({ to: "/setup" });
    }
  },
  component: lazyRouteComponent(() => import("./pages/login/LoginPage"), "LoginPage"),
});

/** 登录后才能看的页面：侧边导航 + 内容区。没登录时跳登录页（还没初始化时跳初始化页）。 */
const appRoute = createRoute({
  getParentRoute: () => rootRoute,
  id: "app",
  beforeLoad: async ({ context, location }) => {
    try {
      await context.queryClient.ensureQueryData(meQuery);
    } catch (err) {
      if (!isUnauthorized(err)) {
        throw err;
      }
      const status = await context.queryClient
        .fetchQuery(setupQuery)
        .catch(() => ({ initialized: true }));
      if (!status.initialized) {
        throw redirect({ to: "/setup" });
      }
      throw redirect({
        to: "/login",
        search: { redirect: location.href === "/" ? undefined : location.href },
      });
    }
  },
  component: AppLayout,
});

const overviewRoute = createRoute({
  getParentRoute: () => appRoute,
  path: "/",
  component: lazyRouteComponent(() => import("./pages/overview/OverviewPage"), "OverviewPage"),
});

const serversRoute = createRoute({
  getParentRoute: () => appRoute,
  path: "/servers",
  component: lazyRouteComponent(() => import("./pages/servers/ServersPage"), "ServersPage"),
});

const serverDetailRoute = createRoute({
  getParentRoute: () => appRoute,
  path: "/servers/$serverId",
  component: lazyRouteComponent(
    () => import("./pages/servers/ServerDetailPage"),
    "ServerDetailPage",
  ),
});

const nodesRoute = createRoute({
  getParentRoute: () => appRoute,
  path: "/nodes",
  component: lazyRouteComponent(() => import("./pages/nodes/NodesPage"), "NodesPage"),
});

const exitsRoute = createRoute({
  getParentRoute: () => appRoute,
  path: "/exits",
  component: lazyRouteComponent(() => import("./pages/exits/ExitsPage"), "ExitsPage"),
});

const plansRoute = createRoute({
  getParentRoute: () => appRoute,
  path: "/plans",
  component: lazyRouteComponent(() => import("./pages/plans/PlansPage"), "PlansPage"),
});

const usersRoute = createRoute({
  getParentRoute: () => appRoute,
  path: "/users",
  component: lazyRouteComponent(() => import("./pages/users/UsersPage"), "UsersPage"),
});

const userDetailRoute = createRoute({
  getParentRoute: () => appRoute,
  path: "/users/$userId",
  component: lazyRouteComponent(() => import("./pages/users/UserDetailPage"), "UserDetailPage"),
});

const trafficRoute = createRoute({
  getParentRoute: () => appRoute,
  path: "/traffic",
  component: lazyRouteComponent(() => import("./pages/traffic/TrafficPage"), "TrafficPage"),
});

const templateRoute = createRoute({
  getParentRoute: () => appRoute,
  path: "/template",
  component: lazyRouteComponent(() => import("./pages/template/TemplatePage"), "TemplatePage"),
});

const settingsRoute = createRoute({
  getParentRoute: () => appRoute,
  path: "/settings",
  component: lazyRouteComponent(() => import("./pages/settings/SettingsPage"), "SettingsPage"),
});

const routeTree = rootRoute.addChildren([
  setupRoute,
  loginRoute,
  appRoute.addChildren([
    overviewRoute,
    serversRoute,
    serverDetailRoute,
    nodesRoute,
    exitsRoute,
    plansRoute,
    usersRoute,
    userDetailRoute,
    trafficRoute,
    templateRoute,
    settingsRoute,
  ]),
]);

export const router = createRouter({
  routeTree,
  context: { queryClient },
  defaultPreload: false,
  // 页面按需加载（每个页面一个代码块），加载超过 300 毫秒才显示转圈
  defaultPendingComponent: () => <LoadingBlock />,
  defaultPendingMs: 300,
  scrollRestoration: true,
});

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}

// TanStack Query 的查询定义。改动类请求成功后会刷新全部查询（见 main.tsx 的 MutationCache），
// 资源之间互相引用（节点显示服务器名、套餐引用节点……），数据量又小，全部刷新最省心。

import { queryOptions } from "@tanstack/react-query";
import {
  authApi,
  type DateRange,
  exitsApi,
  nodesApi,
  plansApi,
  serversApi,
  settingsApi,
  setupApi,
  statsApi,
  templateApi,
  usersApi,
} from "./endpoints";
import type { SubFormat, TrafficGroupBy } from "./types";

/** 服务器列表的刷新间隔：和 Agent 上报网速的周期一致（api.md） */
export const SERVER_REFRESH_MS = 10_000;

export const setupQuery = queryOptions({
  queryKey: ["setup"],
  queryFn: ({ signal }) => setupApi.status(signal),
  staleTime: 0,
});

/** 当前登录的管理员。没登录时的跳转由路由自己处理（router.tsx），不走全局的 401 处理 */
export const meQuery = queryOptions({
  queryKey: ["me"],
  queryFn: ({ signal }) => authApi.me(signal),
  staleTime: 60_000,
  retry: false,
  meta: { skipAuthRedirect: true },
});

export const settingsQuery = queryOptions({
  queryKey: ["settings"],
  queryFn: ({ signal }) => settingsApi.get(signal),
  staleTime: 60_000,
});

export const overviewQuery = queryOptions({
  queryKey: ["overview"],
  queryFn: ({ signal }) => statsApi.overview(signal),
  refetchInterval: 60_000,
});

export const serversQuery = queryOptions({
  queryKey: ["servers"],
  queryFn: ({ signal }) => serversApi.list(signal),
  refetchInterval: SERVER_REFRESH_MS,
});

export const serverQuery = (id: number) =>
  queryOptions({
    queryKey: ["servers", id],
    queryFn: ({ signal }) => serversApi.get(id, signal),
    refetchInterval: SERVER_REFRESH_MS,
  });

export const serverTrafficQuery = (id: number, range: DateRange) =>
  queryOptions({
    queryKey: ["servers", id, "traffic", range.from, range.to],
    queryFn: ({ signal }) => statsApi.server(id, range, signal),
  });

export const realityScanQuery = (id: number) =>
  queryOptions({
    queryKey: ["servers", id, "reality-scan"],
    queryFn: ({ signal }) => serversApi.realityScanStatus(id, signal),
    retry: false,
  });

export const nodesQuery = queryOptions({
  queryKey: ["nodes"],
  queryFn: ({ signal }) => nodesApi.list(signal),
});

export const exitsQuery = queryOptions({
  queryKey: ["exits"],
  queryFn: ({ signal }) => exitsApi.list(signal),
});

export const plansQuery = queryOptions({
  queryKey: ["plans"],
  queryFn: ({ signal }) => plansApi.list(signal),
});

/** 用户列表也定时刷新：停用 / 恢复是否已同步到各服务器（pending_servers）会变 */
export const usersQuery = queryOptions({
  queryKey: ["users"],
  queryFn: ({ signal }) => usersApi.list(signal),
  refetchInterval: SERVER_REFRESH_MS,
});

export const userQuery = (id: number) =>
  queryOptions({
    queryKey: ["users", id],
    queryFn: ({ signal }) => usersApi.get(id, signal),
    refetchInterval: SERVER_REFRESH_MS,
  });

export const userTrafficQuery = (id: number, range: DateRange) =>
  queryOptions({
    queryKey: ["users", id, "traffic", range.from, range.to],
    queryFn: ({ signal }) => statsApi.user(id, range, signal),
  });

export const trafficQuery = (groupBy: TrafficGroupBy, range: DateRange) =>
  queryOptions({
    queryKey: ["traffic", groupBy, range.from, range.to],
    queryFn: ({ signal }) => statsApi.traffic(groupBy, range, signal),
  });

export const templateQuery = queryOptions({
  queryKey: ["template"],
  queryFn: ({ signal }) => templateApi.get(signal),
});

export const templateReportQuery = queryOptions({
  queryKey: ["template", "report"],
  queryFn: ({ signal }) => templateApi.report(signal),
});

export const templatePreviewQuery = (format: SubFormat, userId: number) =>
  queryOptions({
    queryKey: ["template", "preview", format, userId],
    queryFn: ({ signal }) => templateApi.preview(format, userId, signal),
    staleTime: 0,
  });

// 全局的 QueryClient：401 统一跳登录页；改动类请求失败时弹出主控给的错误信息，
// 成功后刷新全部查询（数据量小，资源之间又互相引用）。
// 单个请求可以用 meta 关掉：改动类请求 meta: { silent: true } 不弹错误，{ keepCache: true } 不刷新，
// { allowUnauthorized: true } 的 401 不跳登录页（登录本身）；查询 meta: { skipAuthRedirect: true } 同理。

import { toast } from "@heroui/react";
import { MutationCache, QueryCache, QueryClient } from "@tanstack/react-query";
import { ApiError, errorMessage, isUnauthorized } from "./api/client";

let unauthorizedHandler: () => void = () => {};

/** 由 main.tsx 在建好路由后设置：清掉缓存并跳到登录页。 */
export function setUnauthorizedHandler(handler: () => void) {
  unauthorizedHandler = handler;
}

export const queryClient: QueryClient = new QueryClient({
  queryCache: new QueryCache({
    onError: (error, query) => {
      if (isUnauthorized(error) && query.meta?.skipAuthRedirect !== true) {
        unauthorizedHandler();
      }
    },
  }),
  mutationCache: new MutationCache({
    onError: (error, _variables, _result, mutation) => {
      if (isUnauthorized(error) && mutation.meta?.allowUnauthorized !== true) {
        unauthorizedHandler();
        return;
      }
      if (mutation.meta?.silent !== true) {
        toast.danger(errorMessage(error));
      }
    },
    onSuccess: (_data, _variables, _result, mutation) => {
      if (mutation.meta?.keepCache !== true) {
        void queryClient.invalidateQueries();
      }
    },
  }),
  defaultOptions: {
    queries: {
      staleTime: 5_000,
      // 4xx（参数不对、没登录、不存在）重试也没用；网络错误和 5xx 重试两次
      retry: (failureCount, error) => {
        if (error instanceof ApiError && error.status >= 400 && error.status < 500) {
          return false;
        }
        return failureCount < 2;
      },
    },
  },
});

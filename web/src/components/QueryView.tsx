import { Alert, Button, Spinner } from "@heroui/react";
import type { UseQueryResult } from "@tanstack/react-query";
import type { ReactNode } from "react";
import { errorMessage } from "../api/client";

export function LoadingBlock({ label = "加载中…" }: { label?: string }) {
  return (
    <div className="flex items-center justify-center gap-2 py-16 text-sm text-muted">
      <Spinner size="sm" />
      {label}
    </div>
  );
}

export function ErrorBlock({ error, onRetry }: { error: unknown; onRetry?: () => void }) {
  return (
    <Alert status="danger">
      <Alert.Indicator />
      <Alert.Content>
        <Alert.Title>加载失败</Alert.Title>
        <Alert.Description>{errorMessage(error)}</Alert.Description>
      </Alert.Content>
      {onRetry ? (
        <Button size="sm" variant="danger-soft" onPress={onRetry}>
          重试
        </Button>
      ) : null}
    </Alert>
  );
}

interface QueryViewProps<T> {
  query: UseQueryResult<T>;
  children: (data: T) => ReactNode;
  loading?: ReactNode;
}

/** 按查询状态显示：有数据就渲染，第一次加载显示转圈，失败显示错误和重试。 */
export function QueryView<T>({ query, children, loading }: QueryViewProps<T>) {
  if (query.data !== undefined) {
    return <>{children(query.data)}</>;
  }
  if (query.isError) {
    return <ErrorBlock error={query.error} onRetry={() => void query.refetch()} />;
  }
  return <>{loading ?? <LoadingBlock />}</>;
}

/** 表格没有数据时的提示。 */
export function EmptyHint({ children }: { children: ReactNode }) {
  return <div className="py-10 text-center text-sm text-muted">{children}</div>;
}

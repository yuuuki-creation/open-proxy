import { Button } from "@heroui/react";
import { type ErrorComponentProps, Link } from "@tanstack/react-router";
import { errorMessage } from "../api/client";

export function NotFoundPage() {
  return (
    <div className="flex min-h-[60vh] flex-col items-center justify-center gap-3 text-center">
      <h1 className="text-2xl font-semibold text-foreground">页面不存在</h1>
      <p className="text-sm text-muted">地址可能写错了，或者这个对象已经被删除。</p>
      <Link to="/" className="text-sm text-accent hover:underline">
        回到概览
      </Link>
    </div>
  );
}

export function RouteErrorPage({ error, reset }: ErrorComponentProps) {
  return (
    <div className="flex min-h-[60vh] flex-col items-center justify-center gap-3 text-center">
      <h1 className="text-2xl font-semibold text-foreground">出错了</h1>
      <p className="max-w-lg text-sm text-muted">{errorMessage(error)}</p>
      <div className="flex gap-2">
        <Button variant="secondary" onPress={() => reset()}>
          重试
        </Button>
        <Button variant="tertiary" onPress={() => window.location.assign("/")}>
          回到首页
        </Button>
      </div>
    </div>
  );
}

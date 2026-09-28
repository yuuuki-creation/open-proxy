import {
  ArrowRightFromSquare,
  ChartColumn,
  Cubes3,
  FileCode,
  Gear,
  House,
  Persons,
  Route,
  Server,
  Ticket,
} from "@gravity-ui/icons";
import { Button } from "@heroui/react";
import { useMutation, useQuery } from "@tanstack/react-query";
import { Link, Outlet, useNavigate } from "@tanstack/react-router";
import type { ComponentType, SVGProps } from "react";
import { authApi } from "../api/endpoints";
import { meQuery, settingsQuery } from "../api/queries";
import { queryClient } from "../queryClient";

interface NavItem {
  to:
    | "/"
    | "/servers"
    | "/nodes"
    | "/exits"
    | "/plans"
    | "/users"
    | "/traffic"
    | "/template"
    | "/settings";
  label: string;
  icon: ComponentType<SVGProps<SVGSVGElement>>;
}

const NAV: NavItem[] = [
  { to: "/", label: "概览", icon: House },
  { to: "/servers", label: "服务器", icon: Server },
  { to: "/nodes", label: "节点", icon: Cubes3 },
  { to: "/exits", label: "落地出口", icon: Route },
  { to: "/plans", label: "套餐", icon: Ticket },
  { to: "/users", label: "用户", icon: Persons },
  { to: "/traffic", label: "流量统计", icon: ChartColumn },
  { to: "/template", label: "订阅模板", icon: FileCode },
  { to: "/settings", label: "设置", icon: Gear },
];

/** 登录后的整体布局：左边固定的导航栏，右边是页面内容。 */
export function AppLayout() {
  const navigate = useNavigate();
  const me = useQuery(meQuery);
  const settings = useQuery(settingsQuery);
  const logout = useMutation({
    mutationFn: authApi.logout,
    meta: { keepCache: true, silent: true },
    onSettled: async () => {
      queryClient.clear();
      await navigate({ to: "/login" });
    },
  });

  return (
    <div className="flex min-h-screen bg-background">
      <aside className="sticky top-0 flex h-screen w-56 shrink-0 flex-col border-r border-separator bg-surface px-3 py-5">
        <div className="mb-6 flex items-center gap-2 px-3">
          <img src="/favicon.svg" alt="" className="size-7" />
          <div className="leading-tight">
            <div className="text-sm font-semibold text-foreground">open-proxy</div>
            <div className="text-xs text-muted">管理面板</div>
          </div>
        </div>
        <nav className="flex flex-1 flex-col gap-0.5 overflow-y-auto" aria-label="主导航">
          {NAV.map((item) => (
            <Link
              key={item.to}
              to={item.to}
              activeOptions={{ exact: item.to === "/" }}
              className="flex items-center gap-2.5 rounded-xl px-3 py-2 text-sm transition-colors"
              activeProps={{ className: "bg-accent-soft font-medium text-accent-soft-foreground" }}
              inactiveProps={{ className: "text-muted hover:bg-default hover:text-foreground" }}
            >
              <item.icon className="size-4 shrink-0" />
              {item.label}
            </Link>
          ))}
        </nav>
        <div className="mt-4 flex flex-col gap-2 border-t border-separator px-3 pt-4">
          <div className="text-sm text-foreground">{me.data?.username ?? " "}</div>
          <div className="text-xs text-muted">主控版本 {settings.data?.version ?? "-"}</div>
          <Button
            size="sm"
            variant="ghost"
            className="justify-start px-0"
            isDisabled={logout.isPending}
            onPress={() => logout.mutate()}
          >
            <ArrowRightFromSquare className="size-4" />
            退出登录
          </Button>
        </div>
      </aside>
      <main className="min-w-0 flex-1 px-8 py-7">
        <div className="mx-auto max-w-[1440px]">
          <Outlet />
        </div>
      </main>
    </div>
  );
}

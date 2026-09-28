import { ArrowDown, ArrowUp } from "@gravity-ui/icons";
import { Table } from "@heroui/react";
import { useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import type { ReactNode } from "react";
import { overviewQuery } from "../../api/queries";
import type { Overview, UpDown } from "../../api/types";
import { DOWN_COLOR, StackedBarChart, UP_COLOR } from "../../components/charts";
import { PageHeader, Section } from "../../components/PageHeader";
import { EmptyHint, QueryView } from "../../components/QueryView";
import { UsageBar } from "../../components/UsageBar";
import { formatBytes } from "../../lib/format";

/** 概览：今天、本月的总流量，用户用量排行，最近 30 天趋势，服务器和用户数。 */
export function OverviewPage() {
  const overview = useQuery(overviewQuery);
  return (
    <>
      <PageHeader title="概览" description="流量按管理员时区分日；服务器的实时状态在服务器页看。" />
      <QueryView query={overview}>{(data) => <OverviewContent data={data} />}</QueryView>
    </>
  );
}

function OverviewContent({ data }: { data: Overview }) {
  const series = [
    { key: "down", name: "下载", color: DOWN_COLOR },
    { key: "up", name: "上传", color: UP_COLOR },
  ];
  return (
    <div className="flex flex-col gap-6">
      <div className="grid grid-cols-2 gap-4 xl:grid-cols-4">
        <TrafficTile title="今天" value={data.today} />
        <TrafficTile title="本月" value={data.month} />
        <StatTile
          title="服务器"
          value={`${data.servers.online} / ${data.servers.total}`}
          note={
            data.servers.total === 0
              ? "还没有服务器"
              : data.servers.online === data.servers.total
                ? "全部在线"
                : `${data.servers.total - data.servers.online} 台离线`
          }
          link={<Link to="/servers">查看服务器</Link>}
        />
        <StatTile
          title="用户"
          value={String(data.users.total)}
          note={data.users.blocked > 0 ? `${data.users.blocked} 人已停用` : "没有停用的用户"}
          link={<Link to="/users">查看用户</Link>}
        />
      </div>

      <Section title="最近 30 天每日流量" description="所有用户的上传和下载合计">
        <StackedBarChart
          rows={data.daily.map((d) => ({ day: d.day, down: d.down, up: d.up }))}
          series={series}
        />
      </Section>

      <Section title="本周期用量排行" description="按每个用户自己的重置周期计算，最多显示 10 人">
        <Table variant="secondary">
          <Table.ScrollContainer>
            <Table.Content aria-label="用户用量排行">
              <Table.Header>
                <Table.Column isRowHeader>用户</Table.Column>
                <Table.Column className="w-[45%]">已用 / 额度</Table.Column>
              </Table.Header>
              <Table.Body renderEmptyState={() => <EmptyHint>还没有用户</EmptyHint>}>
                {data.top_users.map((u) => (
                  <Table.Row key={u.id} id={u.id}>
                    <Table.Cell>
                      <Link
                        to="/users/$userId"
                        params={{ userId: String(u.id) }}
                        className="font-medium text-foreground hover:text-accent"
                      >
                        {u.name}
                      </Link>
                    </Table.Cell>
                    <Table.Cell>
                      <UsageBar used={u.used_bytes} quota={u.quota_bytes} />
                    </Table.Cell>
                  </Table.Row>
                ))}
              </Table.Body>
            </Table.Content>
          </Table.ScrollContainer>
        </Table>
      </Section>
    </div>
  );
}

function TrafficTile({ title, value }: { title: string; value: UpDown }) {
  return (
    <div className="rounded-2xl bg-surface p-5 shadow-xs">
      <div className="text-sm text-muted">{title}流量</div>
      <div className="mt-2 text-2xl font-semibold text-foreground">
        {formatBytes(value.up + value.down)}
      </div>
      <div className="mt-2 flex gap-4 text-xs text-muted">
        <span className="inline-flex items-center gap-1">
          <ArrowDown className="size-3.5" />
          下载 {formatBytes(value.down)}
        </span>
        <span className="inline-flex items-center gap-1">
          <ArrowUp className="size-3.5" />
          上传 {formatBytes(value.up)}
        </span>
      </div>
    </div>
  );
}

function StatTile({
  title,
  value,
  note,
  link,
}: {
  title: string;
  value: string;
  note: string;
  link: ReactNode;
}) {
  return (
    <div className="rounded-2xl bg-surface p-5 shadow-xs">
      <div className="text-sm text-muted">{title}</div>
      <div className="mt-2 text-2xl font-semibold text-foreground">{value}</div>
      <div className="mt-2 flex items-center justify-between text-xs text-muted">
        <span>{note}</span>
        <span className="text-accent hover:underline">{link}</span>
      </div>
    </div>
  );
}

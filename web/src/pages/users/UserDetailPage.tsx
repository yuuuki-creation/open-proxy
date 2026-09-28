import { ArrowLeft, Copy } from "@gravity-ui/icons";
import { Button, Disclosure, Table } from "@heroui/react";
import { useQuery } from "@tanstack/react-query";
import { Link, useNavigate, useParams } from "@tanstack/react-router";
import { userQuery, userTrafficQuery } from "../../api/queries";
import type { User, UserTrafficRow } from "../../api/types";
import { CopyBlock } from "../../components/CopyBlock";
import {
  buildEntityChart,
  type EntitySeries,
  StackedBarChart,
  Swatch,
} from "../../components/charts";
import { DateRangeBar, useDateRange, useToday } from "../../components/DateRangeBar";
import { InfoList } from "../../components/InfoList";
import { PageHeader, Section } from "../../components/PageHeader";
import { EmptyHint, QueryView } from "../../components/QueryView";
import { UsageBar } from "../../components/UsageBar";
import { formatBytes, formatDateTime } from "../../lib/format";
import { FORMAT_LABELS, FORMATS } from "../../lib/labels";
import { ExpiresOn, UserStatus, useUserDialogs } from "./userParts";

/** 用户详情：用量、到期、订阅链接，以及按天、按节点的流量明细。 */
export function UserDetailPage() {
  const params = useParams({ strict: false });
  const id = Number(params.userId);
  const navigate = useNavigate();
  const user = useQuery({ ...userQuery(id), enabled: Number.isInteger(id) });
  const actions = useUserDialogs({ onDeleted: () => void navigate({ to: "/users" }) });

  return (
    <>
      <QueryView query={user}>
        {(u) => (
          <>
            <PageHeader
              back={
                <Link
                  to="/users"
                  className="inline-flex items-center gap-1 text-muted hover:text-foreground"
                >
                  <ArrowLeft className="size-3.5" />
                  用户
                </Link>
              }
              title={u.name}
              description={u.remark || undefined}
              actions={
                <>
                  <Button variant="secondary" onPress={() => actions.copyLink(u)}>
                    <Copy />
                    复制订阅链接
                  </Button>
                  <Button
                    variant="tertiary"
                    onPress={() => actions.open({ kind: "edit", user: u })}
                  >
                    编辑
                  </Button>
                  <Button
                    variant="tertiary"
                    onPress={() => actions.open({ kind: "renew", user: u })}
                  >
                    续期
                  </Button>
                  <Button
                    variant="tertiary"
                    onPress={() => actions.open({ kind: "reset-period", user: u })}
                  >
                    清零本周期
                  </Button>
                  <Button variant="tertiary" onPress={() => actions.toggleEnabled(u)}>
                    {u.enabled ? "停用" : "启用"}
                  </Button>
                  <Button
                    variant="danger-soft"
                    onPress={() => actions.open({ kind: "reset-credentials", user: u })}
                  >
                    重置凭据
                  </Button>
                  <Button
                    variant="danger-soft"
                    onPress={() => actions.open({ kind: "delete", user: u })}
                  >
                    删除
                  </Button>
                </>
              }
            />
            <div className="flex flex-col gap-6">
              <UserInfo user={u} />
              <UserTraffic userId={u.id} />
            </div>
          </>
        )}
      </QueryView>
      {actions.dialog}
    </>
  );
}

function UserInfo({ user: u }: { user: User }) {
  const today = useToday();
  const day = Number(u.started_on.slice(8, 10));
  return (
    <Section title="基本信息">
      <InfoList
        items={[
          { label: "套餐", value: u.plan_name },
          { label: "状态", value: <UserStatus user={u} /> },
          {
            label: "本周期已用",
            value: (
              <div className="max-w-sm">
                <UsageBar used={u.used_bytes} quota={u.quota_bytes} label="本周期用量" />
              </div>
            ),
          },
          { label: "到期", value: <ExpiresOn user={u} today={today} /> },
          { label: "开通日", value: `${u.started_on}（每月 ${day} 号重置用量）` },
          {
            label: "累计",
            value: `上传 ${formatBytes(u.up_total)}，下载 ${formatBytes(u.down_total)}`,
          },
          { label: "添加时间", value: formatDateTime(u.created_at) },
          {
            label: "订阅链接",
            wide: true,
            value: u.sub_url ? (
              <div className="flex flex-col gap-2">
                <CopyBlock value={u.sub_url} what="订阅链接" />
                <p className="text-xs text-muted">
                  按客户端的 User-Agent 自动识别格式。识别不对时，可以用下面指定格式的链接。
                </p>
                <Disclosure>
                  <Disclosure.Heading>
                    <Button slot="trigger" size="sm" variant="ghost">
                      指定格式的链接
                      <Disclosure.Indicator />
                    </Button>
                  </Disclosure.Heading>
                  <Disclosure.Content>
                    <Disclosure.Body className="flex flex-col gap-3 pt-2">
                      {FORMATS.map((f) => (
                        <CopyBlock
                          key={f}
                          label={FORMAT_LABELS[f]}
                          value={`${u.sub_url}?format=${f}`}
                          what={`${FORMAT_LABELS[f]} 订阅链接`}
                        />
                      ))}
                    </Disclosure.Body>
                  </Disclosure.Content>
                </Disclosure>
              </div>
            ) : (
              <span className="text-warning">主控还没有域名，先在设置里填写主控域名。</span>
            ),
          },
        ]}
      />
    </Section>
  );
}

function groupByNode(rows: UserTrafficRow[]): EntitySeries[] {
  const map = new Map<number, EntitySeries>();
  for (const r of rows) {
    let entry = map.get(r.node_id);
    if (!entry) {
      entry = { id: r.node_id, name: r.node_name, points: [] };
      map.set(r.node_id, entry);
    }
    entry.points.push({ day: r.day, up: r.up, down: r.down });
  }
  return [...map.values()];
}

function UserTraffic({ userId }: { userId: number }) {
  const [range, setRange, today] = useDateRange();
  const traffic = useQuery(userTrafficQuery(userId, range));
  return (
    <Section title="流量明细" description="按天、按节点；上传 + 下载合计。按管理员时区分日。">
      <div className="mb-4">
        <DateRangeBar value={range} onChange={setRange} today={today} />
      </div>
      <QueryView query={traffic}>
        {(data) => {
          const { rows, series, totals } = buildEntityChart(
            groupByNode(data.rows),
            data.from,
            data.to,
          );
          const byDay = [...data.rows].sort((a, b) =>
            a.day === b.day ? a.node_name.localeCompare(b.node_name) : b.day.localeCompare(a.day),
          );
          return (
            <div className="flex flex-col gap-5">
              <StackedBarChart rows={rows} series={series} />
              <Table variant="secondary">
                <Table.ScrollContainer>
                  <Table.Content aria-label="按节点合计">
                    <Table.Header>
                      <Table.Column isRowHeader>节点</Table.Column>
                      <Table.Column className="text-end">上传</Table.Column>
                      <Table.Column className="text-end">下载</Table.Column>
                      <Table.Column className="text-end">合计</Table.Column>
                    </Table.Header>
                    <Table.Body renderEmptyState={() => <EmptyHint>这段时间没有流量</EmptyHint>}>
                      {totals.map((t) => (
                        <Table.Row key={t.id} id={t.id}>
                          <Table.Cell>
                            <span className="inline-flex items-center gap-2">
                              <Swatch color={t.color} />
                              {t.name}
                            </span>
                          </Table.Cell>
                          <Table.Cell className="text-end tabular-nums">
                            {formatBytes(t.up)}
                          </Table.Cell>
                          <Table.Cell className="text-end tabular-nums">
                            {formatBytes(t.down)}
                          </Table.Cell>
                          <Table.Cell className="text-end font-medium tabular-nums">
                            {formatBytes(t.up + t.down)}
                          </Table.Cell>
                        </Table.Row>
                      ))}
                    </Table.Body>
                  </Table.Content>
                </Table.ScrollContainer>
              </Table>
              {byDay.length > 0 ? (
                <Disclosure>
                  <Disclosure.Heading>
                    <Button slot="trigger" size="sm" variant="ghost">
                      按天明细（{byDay.length} 条）
                      <Disclosure.Indicator />
                    </Button>
                  </Disclosure.Heading>
                  <Disclosure.Content>
                    <Disclosure.Body className="pt-2">
                      <Table variant="secondary">
                        <Table.ScrollContainer>
                          <Table.Content aria-label="按天明细">
                            <Table.Header>
                              <Table.Column isRowHeader>日期</Table.Column>
                              <Table.Column>节点</Table.Column>
                              <Table.Column className="text-end">上传</Table.Column>
                              <Table.Column className="text-end">下载</Table.Column>
                            </Table.Header>
                            <Table.Body>
                              {byDay.map((r) => (
                                <Table.Row
                                  key={`${r.day}-${r.node_id}`}
                                  id={`${r.day}-${r.node_id}`}
                                >
                                  <Table.Cell className="tabular-nums">{r.day}</Table.Cell>
                                  <Table.Cell>{r.node_name}</Table.Cell>
                                  <Table.Cell className="text-end tabular-nums">
                                    {formatBytes(r.up)}
                                  </Table.Cell>
                                  <Table.Cell className="text-end tabular-nums">
                                    {formatBytes(r.down)}
                                  </Table.Cell>
                                </Table.Row>
                              ))}
                            </Table.Body>
                          </Table.Content>
                        </Table.ScrollContainer>
                      </Table>
                    </Disclosure.Body>
                  </Disclosure.Content>
                </Disclosure>
              ) : null}
            </div>
          );
        }}
      </QueryView>
    </Section>
  );
}

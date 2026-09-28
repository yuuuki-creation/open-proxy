import { Table, Tabs } from "@heroui/react";
import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import type { DateRange } from "../../api/endpoints";
import { trafficQuery } from "../../api/queries";
import type { TrafficGroupBy } from "../../api/types";
import { buildEntityChart, StackedBarChart, Swatch } from "../../components/charts";
import { DateRangeBar, useDateRange } from "../../components/DateRangeBar";
import { PageHeader, Section } from "../../components/PageHeader";
import { EmptyHint, QueryView } from "../../components/QueryView";
import { formatBytes } from "../../lib/format";

const GROUPS: { id: TrafficGroupBy; label: string; noun: string; note: string }[] = [
  { id: "user", label: "按用户", noun: "用户", note: "每个用户每天用了多少（上传 + 下载）。" },
  { id: "node", label: "按节点", noun: "节点", note: "每个节点每天走了多少用户流量。" },
  {
    id: "server",
    label: "按服务器",
    noun: "服务器",
    note: "经过每台服务器的代理流量；整机网卡流量在服务器详情里看。",
  },
];

/** 流量统计：按用户、节点、服务器分组的每日流量。 */
export function TrafficPage() {
  const [group, setGroup] = useState<TrafficGroupBy>("user");
  const [range, setRange, today] = useDateRange();

  return (
    <>
      <PageHeader
        title="流量统计"
        description="按管理员时区分日。已删除的节点和服务器的历史流量保留。"
      />
      <Tabs
        selectedKey={group}
        onSelectionChange={(key) => setGroup(String(key) as TrafficGroupBy)}
      >
        <div className="flex flex-wrap items-center justify-between gap-4">
          <Tabs.ListContainer>
            <Tabs.List aria-label="统计维度">
              {GROUPS.map((g) => (
                <Tabs.Tab key={g.id} id={g.id}>
                  {g.label}
                  <Tabs.Indicator />
                </Tabs.Tab>
              ))}
            </Tabs.List>
          </Tabs.ListContainer>
          <DateRangeBar value={range} onChange={setRange} today={today} />
        </div>
        {GROUPS.map((g) => (
          <Tabs.Panel key={g.id} id={g.id} className="pt-4">
            <TrafficContent group={g.id} range={range} noun={g.noun} note={g.note} />
          </Tabs.Panel>
        ))}
      </Tabs>
    </>
  );
}

function TrafficContent({
  group,
  range,
  noun,
  note,
}: {
  group: TrafficGroupBy;
  range: DateRange;
  noun: string;
  note: string;
}) {
  const traffic = useQuery(trafficQuery(group, range));
  return (
    <QueryView query={traffic}>
      {(data) => {
        const { rows, series, totals } = buildEntityChart(data.series, data.from, data.to);
        const sum = totals.reduce((s, t) => s + t.up + t.down, 0);
        return (
          <div className="flex flex-col gap-6">
            <Section
              title={`每日流量（${noun}）`}
              description={`${note}超过 8 个${noun}时只单独画用量最大的 7 个，其余合并成「其他」。`}
            >
              <StackedBarChart rows={rows} series={series} height={320} />
            </Section>
            <Section title={`${data.from} 至 ${data.to} 合计 ${formatBytes(sum)}`}>
              <Table variant="secondary">
                <Table.ScrollContainer>
                  <Table.Content aria-label={`按${noun}合计`}>
                    <Table.Header>
                      <Table.Column isRowHeader>{noun}</Table.Column>
                      <Table.Column className="text-end">上传</Table.Column>
                      <Table.Column className="text-end">下载</Table.Column>
                      <Table.Column className="text-end">合计</Table.Column>
                      <Table.Column className="text-end">占比</Table.Column>
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
                          <Table.Cell className="text-end text-muted tabular-nums">
                            {sum > 0 ? `${(((t.up + t.down) / sum) * 100).toFixed(1)}%` : "-"}
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
      }}
    </QueryView>
  );
}

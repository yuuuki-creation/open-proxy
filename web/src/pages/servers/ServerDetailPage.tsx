import { ArrowLeft } from "@gravity-ui/icons";
import { Button, Chip, Table } from "@heroui/react";
import { useQuery } from "@tanstack/react-query";
import { Link, useNavigate, useParams } from "@tanstack/react-router";
import { nodesQuery, serverQuery, serverTrafficQuery } from "../../api/queries";
import type { Server } from "../../api/types";
import { DOWN_COLOR, fillDays, StackedBarChart, UP_COLOR } from "../../components/charts";
import { DateRangeBar, useDateRange } from "../../components/DateRangeBar";
import { InfoList } from "../../components/InfoList";
import { PageHeader, Section } from "../../components/PageHeader";
import { EmptyHint, LoadingBlock, QueryView } from "../../components/QueryView";
import { RealityTools } from "../../components/reality";
import { UsageBar } from "../../components/UsageBar";
import { formatBytes, formatDateTime, formatQuota, formatRelative } from "../../lib/format";
import { CERT_MODE_LABELS, protocolLabel } from "../../lib/labels";
import {
  AgentVersion,
  canUpgrade,
  FailureList,
  ServerStatusChip,
  SpeedCell,
  SyncStatus,
  useServerDialogs,
} from "./serverParts";

/** 服务器详情：基本信息、配置应用失败的项、这台服务器上的节点、网卡每日流量、REALITY 伪装目标工具。 */
export function ServerDetailPage() {
  const params = useParams({ strict: false });
  const id = Number(params.serverId);
  const navigate = useNavigate();
  const server = useQuery({ ...serverQuery(id), enabled: Number.isInteger(id) });
  const { open, dialog, masterVersion } = useServerDialogs({
    onDeleted: () => void navigate({ to: "/servers" }),
  });

  return (
    <>
      <QueryView query={server}>
        {(s) => (
          <>
            <PageHeader
              back={
                <Link
                  to="/servers"
                  className="inline-flex items-center gap-1 text-muted hover:text-foreground"
                >
                  <ArrowLeft className="size-3.5" />
                  服务器
                </Link>
              }
              title={
                <span className="flex items-center gap-3">
                  {s.name}
                  <ServerStatusChip server={s} />
                </span>
              }
              description={s.address}
              actions={
                <>
                  <Button variant="tertiary" onPress={() => open({ kind: "edit", server: s })}>
                    编辑
                  </Button>
                  <Button
                    variant="tertiary"
                    onPress={() => open({ kind: "regenerate", server: s })}
                  >
                    重新生成安装命令
                  </Button>
                  <Button
                    variant={canUpgrade(s, masterVersion) ? "secondary" : "tertiary"}
                    onPress={() => open({ kind: "upgrade", server: s })}
                  >
                    升级 Agent
                  </Button>
                  <Button variant="danger-soft" onPress={() => open({ kind: "delete", server: s })}>
                    删除
                  </Button>
                </>
              }
            />
            <div className="flex flex-col gap-6">
              <ServerInfo server={s} masterVersion={masterVersion} />
              {s.apply_failures.length > 0 ? (
                <Section
                  title="配置应用失败的项"
                  description="Agent 应用期望状态时这些项没成功，其余的照常生效。改好后会自动重试。"
                >
                  <FailureList failures={s.apply_failures} />
                </Section>
              ) : null}
              <ServerNodes serverId={s.id} />
              <ServerTraffic serverId={s.id} />
              <Section
                title="REALITY 伪装目标"
                description="给这台服务器上的 VLESS + REALITY 节点找伪装目标：检测候选网站，或让 Agent 扫描所在网段。"
              >
                <RealityTools serverId={s.id} />
              </Section>
            </div>
          </>
        )}
      </QueryView>
      {dialog}
    </>
  );
}

function ServerInfo({
  server: s,
  masterVersion,
}: {
  server: Server;
  masterVersion: string | undefined;
}) {
  const cert = s.certificate;
  return (
    <Section title="基本信息">
      <InfoList
        items={[
          { label: "地址", value: <span className="font-mono">{s.address}</span> },
          {
            label: "节点端口范围",
            value: `${s.port_range_start}–${s.port_range_end}`,
          },
          {
            label: "证书方式",
            value: (
              <>
                {CERT_MODE_LABELS[s.cert_mode]}
                {s.cert_domain ? <span className="ml-2 font-mono">{s.cert_domain}</span> : null}
              </>
            ),
          },
          {
            label: "证书",
            value: cert ? (
              <div className="flex flex-col gap-0.5">
                <span>
                  {cert.kind === "acme" ? "已申请" : "自签"}，{formatDateTime(cert.not_after)} 到期
                </span>
                <span className="break-all font-mono text-xs text-muted">
                  SHA-256 {cert.sha256}
                </span>
                {cert.last_error ? (
                  <span className="text-xs text-danger">上次续期失败：{cert.last_error}</span>
                ) : null}
              </div>
            ) : (
              <span className="text-muted">
                {s.cert_mode === "acme" ? "还没申请到（主控会自动申请）" : "还没有"}
              </span>
            ),
          },
          {
            label: "Agent 版本",
            value: <AgentVersion server={s} masterVersion={masterVersion} />,
          },
          {
            label: "最近在线",
            value: s.online ? "现在在线" : formatRelative(s.last_seen_at),
          },
          { label: "实时网速", value: <SpeedCell server={s} /> },
          { label: "配置同步", value: <SyncStatus server={s} /> },
          {
            label: "本月用量",
            value: (
              <div className="flex max-w-sm flex-col gap-1">
                <UsageBar
                  used={s.month_rx + s.month_tx}
                  quota={s.traffic_quota_bytes}
                  label="本月用量"
                />
                <span className="text-xs text-muted">
                  收 {formatBytes(s.month_rx)}，发 {formatBytes(s.month_tx)}
                </span>
              </div>
            ),
          },
          {
            label: "月额度和重置日",
            value: `${formatQuota(s.traffic_quota_bytes)}，每月 ${s.traffic_reset_day ?? 1} 号重置`,
          },
          { label: "添加时间", value: formatDateTime(s.created_at) },
        ]}
      />
    </Section>
  );
}

function ServerNodes({ serverId }: { serverId: number }) {
  const nodes = useQuery(nodesQuery);
  const list = (nodes.data ?? []).filter((n) => n.server_id === serverId);
  return (
    <Section
      title="这台服务器上的节点"
      actions={
        <Link to="/nodes" className="text-sm text-accent hover:underline">
          去节点页管理
        </Link>
      }
    >
      {nodes.isPending ? (
        <LoadingBlock />
      ) : (
        <Table variant="secondary">
          <Table.ScrollContainer>
            <Table.Content aria-label="这台服务器上的节点">
              <Table.Header>
                <Table.Column isRowHeader>名称</Table.Column>
                <Table.Column>协议</Table.Column>
                <Table.Column>端口</Table.Column>
                <Table.Column>落地出口</Table.Column>
                <Table.Column>状态</Table.Column>
              </Table.Header>
              <Table.Body renderEmptyState={() => <EmptyHint>这台服务器上还没有节点</EmptyHint>}>
                {list.map((n) => (
                  <Table.Row key={n.id} id={n.id}>
                    <Table.Cell className="font-medium">{n.name}</Table.Cell>
                    <Table.Cell>{protocolLabel(n.protocol)}</Table.Cell>
                    <Table.Cell className="font-mono text-xs">
                      {n.port}
                      {n.hop_ports ? ` + ${n.hop_ports.start}–${n.hop_ports.end}` : ""}
                    </Table.Cell>
                    <Table.Cell>{n.exit_name ?? "直连"}</Table.Cell>
                    <Table.Cell>
                      {n.enabled ? (
                        <Chip size="sm" color="success" variant="soft">
                          启用
                        </Chip>
                      ) : (
                        <Chip size="sm" variant="soft">
                          停用
                        </Chip>
                      )}
                    </Table.Cell>
                  </Table.Row>
                ))}
              </Table.Body>
            </Table.Content>
          </Table.ScrollContainer>
        </Table>
      )}
    </Section>
  );
}

function ServerTraffic({ serverId }: { serverId: number }) {
  const [range, setRange, today] = useDateRange();
  const traffic = useQuery(serverTrafficQuery(serverId, range));
  return (
    <Section
      title="网卡每日流量"
      description="整台服务器网卡的收发（包括不经过代理的流量），按管理员时区分日。"
    >
      <div className="mb-4">
        <DateRangeBar value={range} onChange={setRange} today={today} />
      </div>
      <QueryView query={traffic}>
        {(data) => {
          const rows = fillDays(
            data.from,
            data.to,
            data.rows.map((r) => ({ day: r.day, rx: r.rx, tx: r.tx })),
            (day) => ({ day, rx: 0, tx: 0 }),
          );
          const rx = rows.reduce((sum, r) => sum + r.rx, 0);
          const tx = rows.reduce((sum, r) => sum + r.tx, 0);
          return (
            <div className="flex flex-col gap-3">
              <StackedBarChart
                rows={rows}
                series={[
                  { key: "rx", name: "接收", color: DOWN_COLOR },
                  { key: "tx", name: "发送", color: UP_COLOR },
                ]}
              />
              <p className="text-sm text-muted">
                {data.from} 至 {data.to} 合计：接收 {formatBytes(rx)}，发送 {formatBytes(tx)}，共{" "}
                {formatBytes(rx + tx)}
              </p>
            </div>
          );
        }}
      </QueryView>
    </Section>
  );
}

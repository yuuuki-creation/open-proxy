// REALITY 伪装目标：检测（Agent 在服务器上访问目标，看 TLS 1.3 / H2 / 延迟 / 证书）和扫描（Agent 慢速扫所在网段）。
// 接口在主控 P6 实现（nodes.md「REALITY 伪装目标」）：检测最多等 1 分钟；扫描同一台服务器同时只跑一个。

import { CircleCheck, CircleXmark, Magnifier } from "@gravity-ui/icons";
import {
  Alert,
  Button,
  Chip,
  Input,
  Label,
  Spinner,
  Table,
  TextArea,
  TextField,
} from "@heroui/react";
import { useMutation, useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { errorMessage } from "../api/client";
import { serversApi } from "../api/endpoints";
import { realityScanQuery } from "../api/queries";
import type { RealityCheckResult } from "../api/types";
import { formatDateTime } from "../lib/format";
import { isHostname, isIpv4 } from "../lib/validation";
import { ActionButton } from "./ActionButton";
import { ConfirmDialog } from "./ConfirmDialog";
import { EmptyHint } from "./QueryView";

/** 检测通过：支持 TLS 1.3 和 H2，证书有效，没有错误。 */
export function checkPassed(r: RealityCheckResult): boolean {
  return r.tls13 && r.h2 && r.certificate_valid && !r.error;
}

/** 伪装目标写法统一成「域名:端口」，端口默认 443；不是合法写法时返回 null。 */
export function normalizeTarget(value: string): string | null {
  const v = value.trim().toLowerCase();
  if (!v) {
    return null;
  }
  const colon = v.lastIndexOf(":");
  const host = colon >= 0 ? v.slice(0, colon) : v;
  const port = colon >= 0 ? v.slice(colon + 1) : "443";
  if (!isHostname(host) || !/^\d+$/.test(port) || Number(port) < 1 || Number(port) > 65535) {
    return null;
  }
  return `${host}:${Number(port)}`;
}

/**
 * 从粘贴的文字里取出候选目标：每行一个域名，或者 RealiTLScanner 的 CSV 输出
 * （IP,ORIGIN,CERT_DOMAIN,CERT_ISSUER,GEO_CODE，取其中的域名）。去重，最多 20 个。
 */
export function parseTargets(text: string): string[] {
  const found: string[] = [];
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.trim();
    if (!line) {
      continue;
    }
    const fields = line.includes(",") ? line.split(",").map((f) => f.trim()) : [line];
    const candidate = fields.find((f) => {
      const host = f.replace(/^\*\./, "");
      return !isIpv4(host) && normalizeTarget(host) !== null;
    });
    if (candidate) {
      const normalized = normalizeTarget(candidate.replace(/^\*\./, ""));
      if (normalized && !found.includes(normalized)) {
        found.push(normalized);
      }
    }
  }
  return found.slice(0, 20);
}

function Mark({ ok }: { ok: boolean }) {
  return ok ? (
    <CircleCheck className="size-4 text-success" aria-label="是" />
  ) : (
    <CircleXmark className="size-4 text-danger" aria-label="否" />
  );
}

export function CheckResultTable({
  results,
  onPick,
}: {
  results: RealityCheckResult[];
  onPick?: (target: string) => void;
}) {
  return (
    <Table variant="secondary">
      <Table.ScrollContainer>
        <Table.Content aria-label="检测结果" className="min-w-[640px]">
          <Table.Header>
            <Table.Column isRowHeader>目标</Table.Column>
            <Table.Column>TLS 1.3</Table.Column>
            <Table.Column>H2</Table.Column>
            <Table.Column>证书有效</Table.Column>
            <Table.Column>延迟</Table.Column>
            <Table.Column>结果</Table.Column>
          </Table.Header>
          <Table.Body renderEmptyState={() => <EmptyHint>没有结果</EmptyHint>}>
            {results.map((r) => (
              <Table.Row key={r.target} id={r.target}>
                <Table.Cell className="font-mono text-xs">{r.target}</Table.Cell>
                <Table.Cell>
                  <Mark ok={r.tls13} />
                </Table.Cell>
                <Table.Cell>
                  <Mark ok={r.h2} />
                </Table.Cell>
                <Table.Cell>
                  <Mark ok={r.certificate_valid} />
                </Table.Cell>
                <Table.Cell className="tabular-nums">
                  {r.error ? "-" : `${r.latency_ms} ms`}
                </Table.Cell>
                <Table.Cell>
                  {checkPassed(r) ? (
                    onPick ? (
                      <Button size="sm" variant="secondary" onPress={() => onPick(r.target)}>
                        用这个
                      </Button>
                    ) : (
                      <Chip size="sm" color="success" variant="soft">
                        可用
                      </Chip>
                    )
                  ) : (
                    <span className="text-xs text-danger">{r.error || "不满足要求"}</span>
                  )}
                </Table.Cell>
              </Table.Row>
            ))}
          </Table.Body>
        </Table.Content>
      </Table.ScrollContainer>
    </Table>
  );
}

/** 批量检测候选目标：粘贴本地 RealiTLScanner 的结果，或者每行一个域名。 */
function RealityBatchCheck({
  serverId,
  onPick,
  text,
  setText,
}: {
  serverId: number;
  onPick?: (target: string) => void;
  text: string;
  setText: (text: string) => void;
}) {
  const check = useMutation({
    mutationFn: (targets: string[]) => serversApi.realityCheck(serverId, targets),
    meta: { keepCache: true },
  });
  const targets = parseTargets(text);

  return (
    <div className="flex flex-col gap-3">
      <TextField value={text} onChange={setText} fullWidth>
        <Label>候选目标</Label>
        <TextArea
          rows={4}
          className="font-mono text-xs"
          placeholder={
            "每行一个域名，例如 www.example.com\n也可以直接粘贴 RealiTLScanner 输出的 CSV"
          }
          spellCheck={false}
        />
      </TextField>
      <div className="flex items-center gap-3">
        <ActionButton
          size="sm"
          variant="secondary"
          isPending={check.isPending}
          isDisabled={targets.length === 0}
          onPress={() => check.mutate(targets)}
        >
          检测 {targets.length > 0 ? `${targets.length} 个目标` : ""}
        </ActionButton>
        <span className="text-xs text-muted">
          由 Agent 在服务器上访问这些网站，最多等 1 分钟；只是正常的 HTTPS 访问，不算扫描。
        </span>
      </div>
      {check.isPending ? (
        <div className="flex items-center gap-2 text-sm text-muted">
          <Spinner size="sm" />
          正在检测…
        </div>
      ) : null}
      {check.data ? <CheckResultTable results={check.data.results} onPick={onPick} /> : null}
    </div>
  );
}

/** 让 Agent 扫描服务器所在网段（默认 /24），找支持 TLS 1.3 和 H2 的网站。 */
export function RealityScanPanel({
  serverId,
  onCheck,
}: {
  serverId: number;
  /** 把扫描到的域名拿去检测 */
  onCheck?: (domain: string) => void;
}) {
  const [confirming, setConfirming] = useState(false);
  const [cidr, setCidr] = useState("");
  const [concurrency, setConcurrency] = useState("");
  const [rate, setRate] = useState("");
  const scan = useQuery({
    ...realityScanQuery(serverId),
    refetchInterval: (query) => (query.state.data?.status === "running" ? 3_000 : false),
  });
  const start = useMutation({
    mutationFn: () =>
      serversApi.realityScan(serverId, {
        cidr: cidr.trim() || undefined,
        concurrency: concurrency.trim() ? Number(concurrency) : undefined,
        max_per_second: rate.trim() ? Number(rate) : undefined,
      }),
    meta: { keepCache: true },
    onSuccess: () => {
      void scan.refetch();
    },
  });

  const state = scan.data;
  const running = state?.status === "running";

  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-wrap items-center gap-3">
        <Button
          size="sm"
          variant="secondary"
          isDisabled={running || start.isPending}
          onPress={() => setConfirming(true)}
        >
          <Magnifier />让 Agent 扫描
        </Button>
        {scan.isError ? (
          <span className="text-xs text-danger">查询扫描状态失败：{errorMessage(scan.error)}</span>
        ) : null}
        {state ? (
          <span className="inline-flex items-center gap-1.5 text-xs text-muted">
            {running ? <Spinner size="sm" /> : null}
            {state.status === "idle" ? "还没有扫描过" : null}
            {running ? `扫描中（开始于 ${formatDateTime(state.started_at)}）` : null}
            {state.status === "done"
              ? `上次扫描完成于 ${formatDateTime(state.finished_at)}，找到 ${state.candidates.length} 个`
              : null}
            {state.status === "failed" ? `扫描失败：${state.error}` : null}
          </span>
        ) : null}
      </div>
      {state && state.candidates.length > 0 ? (
        <Table variant="secondary">
          <Table.ScrollContainer>
            <Table.Content aria-label="扫描到的候选目标" className="min-w-[640px]">
              <Table.Header>
                <Table.Column isRowHeader>域名</Table.Column>
                <Table.Column>IP</Table.Column>
                <Table.Column>证书签发者</Table.Column>
                <Table.Column>延迟</Table.Column>
                <Table.Column className="text-end">操作</Table.Column>
              </Table.Header>
              <Table.Body>
                {state.candidates.map((c) => (
                  <Table.Row key={`${c.ip}-${c.domain}`} id={`${c.ip}-${c.domain}`}>
                    <Table.Cell className="font-mono text-xs">{c.domain}</Table.Cell>
                    <Table.Cell className="font-mono text-xs">{c.ip}</Table.Cell>
                    <Table.Cell className="text-xs">{c.issuer}</Table.Cell>
                    <Table.Cell className="tabular-nums">{c.latency_ms} ms</Table.Cell>
                    <Table.Cell className="text-end">
                      {onCheck ? (
                        <Button size="sm" variant="ghost" onPress={() => onCheck(c.domain)}>
                          检测
                        </Button>
                      ) : null}
                    </Table.Cell>
                  </Table.Row>
                ))}
              </Table.Body>
            </Table.Content>
          </Table.ScrollContainer>
        </Table>
      ) : null}
      {confirming ? (
        <ConfirmDialog
          isOpen
          onOpenChange={(open) => !open && setConfirming(false)}
          title="让 Agent 扫描所在网段？"
          confirmLabel="我知道风险，开始扫描"
          status="warning"
          onConfirm={() => start.mutateAsync()}
        >
          <Alert status="warning">
            <Alert.Indicator />
            <Alert.Content>
              <Alert.Description>
                在云服务器上扫描网段可能被服务商标记，甚至导致 VPS
                被封。更稳妥的做法是在自己电脑上跑
                RealiTLScanner，把结果粘贴到上面的「候选目标」里检测。
              </Alert.Description>
            </Alert.Content>
          </Alert>
          <div className="grid grid-cols-3 gap-3">
            <TextField value={cidr} onChange={setCidr}>
              <Label>网段</Label>
              <Input placeholder="默认所在的 /24" className="font-mono" />
            </TextField>
            <TextField value={concurrency} onChange={setConcurrency}>
              <Label>并发数</Label>
              <Input placeholder="默认" inputMode="numeric" />
            </TextField>
            <TextField value={rate} onChange={setRate}>
              <Label>每秒最多</Label>
              <Input placeholder="默认" inputMode="numeric" />
            </TextField>
          </div>
          <p className="text-xs text-muted">
            不填就用 Agent 的默认值（慢速、低并发）。结果只放在主控内存里。
          </p>
        </ConfirmDialog>
      ) : null}
    </div>
  );
}

/** 伪装目标工具：检测候选目标 + 让 Agent 扫描。onPick 给了就能把检测通过的目标选进表单。 */
export function RealityTools({
  serverId,
  onPick,
}: {
  serverId: number;
  onPick?: (target: string) => void;
}) {
  const [text, setText] = useState("");
  return (
    <div className="flex flex-col gap-6">
      <div className="flex flex-col gap-2">
        <h3 className="text-sm font-semibold text-foreground">检测候选目标</h3>
        <RealityBatchCheck serverId={serverId} onPick={onPick} text={text} setText={setText} />
      </div>
      <div className="flex flex-col gap-2">
        <h3 className="text-sm font-semibold text-foreground">让 Agent 扫描所在网段</h3>
        <RealityScanPanel
          serverId={serverId}
          onCheck={(domain) => setText(text.trim() ? `${text.trim()}\n${domain}` : domain)}
        />
      </div>
    </div>
  );
}

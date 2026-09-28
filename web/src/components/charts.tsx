// 每日流量的堆叠柱状图（recharts）。配色用固定顺序的 8 色分类色板（已按色弱可辨性校验），
// 超过 8 个对象时只画用量最大的 7 个，其余合并成「其他」；每个图下面都有表格，数值不只靠颜色和悬停提示。

import type { ReactNode } from "react";
import {
  Bar,
  BarChart,
  CartesianGrid,
  ResponsiveContainer,
  Tooltip,
  type TooltipContentProps,
  XAxis,
  YAxis,
} from "recharts";
import { eachDay, shortDay } from "../lib/date";
import { formatBytes } from "../lib/format";

/** 分类色板，顺序固定，不循环使用。 */
export const SERIES_COLORS = [
  "#2a78d6",
  "#eb6834",
  "#1baf7a",
  "#eda100",
  "#e87ba4",
  "#008300",
  "#4a3aa7",
  "#e34948",
];

/** 「其他」用中性灰，不占分类色。 */
export const OTHER_COLOR = "#a3a29c";

/** 上传 / 下载（或网卡收 / 发）两条序列的颜色。 */
export const DOWN_COLOR = SERIES_COLORS[0];
export const UP_COLOR = SERIES_COLORS[1];

export interface ChartSeries {
  key: string;
  name: string;
  color: string;
}

export type ChartRow = { day: string } & Record<string, number | string>;

interface StackedBarChartProps {
  rows: ChartRow[];
  /** 从下往上堆叠的顺序 */
  series: ChartSeries[];
  height?: number;
}

const BYTE_UNITS = [1, 1024, 1024 ** 2, 1024 ** 3, 1024 ** 4, 1024 ** 5];

/**
 * 纵轴刻度：按 1024 进制的单位取整（例如 0 / 2 GB / 4 GB / 6 GB），
 * 不用 recharts 默认的十进制刻度（那样会显示成 476.8 MB 这种）。没有数据时按 1 GB 画。
 */
export function byteTicks(max: number, count = 4): number[] {
  const top = max > 0 ? max : 1024 ** 3;
  const unit = BYTE_UNITS.filter((u) => top >= u).pop() ?? 1;
  const scaled = top / unit;
  const rough = scaled / count;
  const pow = 10 ** Math.floor(Math.log10(rough));
  const step = [1, 2, 2.5, 5, 10].map((m) => m * pow).find((s) => s >= rough) ?? 10 * pow;
  const ticks: number[] = [];
  for (let i = 0; i * step < scaled + step - 1e-9; i += 1) {
    ticks.push(Math.round(i * step * unit));
  }
  return ticks;
}

/** 刻度文字：最多一位小数，去掉多余的 0（「256 MB」「1.5 GB」）。 */
function formatTick(value: number): string {
  return formatBytes(value, 1).replace(/\.0 /, " ");
}

export function StackedBarChart({ rows, series, height = 280 }: StackedBarChartProps) {
  const top = series[series.length - 1]?.key;
  const max = rows.reduce(
    (m, row) =>
      Math.max(
        m,
        series.reduce((sum, s) => sum + Number(row[s.key] ?? 0), 0),
      ),
    0,
  );
  const ticks = byteTicks(max);
  return (
    <div className="flex flex-col gap-3">
      {series.length > 1 ? <ChartLegend series={series} /> : null}
      <div className="w-full" style={{ height }}>
        <ResponsiveContainer width="100%" height="100%">
          <BarChart data={rows} margin={{ top: 8, right: 8, bottom: 0, left: 0 }}>
            <CartesianGrid vertical={false} stroke="var(--separator)" />
            <XAxis
              dataKey="day"
              tickFormatter={(day: string) => shortDay(day)}
              tickLine={false}
              axisLine={{ stroke: "var(--border)" }}
              tick={{ fill: "var(--muted)", fontSize: 12 }}
              minTickGap={12}
            />
            <YAxis
              ticks={ticks}
              domain={[0, ticks[ticks.length - 1] ?? 0]}
              tickFormatter={(value: number) => formatTick(value)}
              tickLine={false}
              axisLine={false}
              tick={{ fill: "var(--muted)", fontSize: 12 }}
              width={76}
            />
            <Tooltip
              cursor={{ fill: "var(--default)", fillOpacity: 0.6 }}
              content={ChartTooltip}
              isAnimationActive={false}
            />
            {series.map((s) => (
              <Bar
                key={s.key}
                dataKey={s.key}
                name={s.name}
                stackId="total"
                fill={s.color}
                stroke="var(--surface)"
                strokeWidth={1}
                maxBarSize={24}
                radius={s.key === top ? [4, 4, 0, 0] : 0}
                isAnimationActive={false}
              />
            ))}
          </BarChart>
        </ResponsiveContainer>
      </div>
    </div>
  );
}

function ChartTooltip({ active, payload, label }: TooltipContentProps) {
  if (!active || !payload || payload.length === 0) {
    return null;
  }
  const entries = [...payload].reverse();
  const total = entries.reduce((sum, entry) => sum + Number(entry.value ?? 0), 0);
  return (
    <div className="min-w-44 rounded-xl bg-overlay px-3 py-2 text-xs text-foreground shadow-lg">
      <div className="mb-1.5 text-muted">{String(label ?? "")}</div>
      <div className="flex flex-col gap-1">
        {entries.map((entry) => (
          <div key={String(entry.dataKey ?? entry.name)} className="flex items-center gap-2">
            <span className="h-0.5 w-3 shrink-0 rounded" style={{ background: entry.color }} />
            <span className="font-semibold tabular-nums">
              {formatBytes(Number(entry.value ?? 0))}
            </span>
            <span className="truncate text-muted">{entry.name}</span>
          </div>
        ))}
      </div>
      {entries.length > 1 ? (
        <div className="mt-1.5 border-t border-separator pt-1.5">
          <span className="font-semibold tabular-nums">{formatBytes(total)}</span>
          <span className="text-muted"> 合计</span>
        </div>
      ) : null}
    </div>
  );
}

export function ChartLegend({ series }: { series: ChartSeries[] }) {
  return (
    <div className="flex flex-wrap gap-x-4 gap-y-1 text-xs text-muted">
      {series.map((s) => (
        <span key={s.key} className="inline-flex items-center gap-1.5">
          <span className="size-2.5 rounded-sm" style={{ background: s.color }} />
          {s.name}
        </span>
      ))}
    </div>
  );
}

/** 把稀疏的「有流量的日子」补成连续的每一天。 */
export function fillDays<T extends { day: string }>(
  from: string,
  to: string,
  points: T[],
  empty: (day: string) => T,
): T[] {
  const byDay = new Map(points.map((p) => [p.day, p]));
  return eachDay(from, to).map((day) => byDay.get(day) ?? empty(day));
}

export interface EntitySeries {
  id: number;
  name: string;
  points: { day: string; up: number; down: number }[];
}

export interface EntityTotal {
  id: number;
  name: string;
  up: number;
  down: number;
  color: string;
}

/**
 * 多个对象（用户、节点、服务器）的每日流量 → 图表数据。最多 8 种颜色：
 * 超过 8 个对象时取总量最大的 7 个，其余合并成「其他」。颜色按对象 ID 顺序分配，范围不变时同一个对象颜色不变。
 */
export function buildEntityChart(
  entities: EntitySeries[],
  from: string,
  to: string,
): { rows: ChartRow[]; series: ChartSeries[]; totals: EntityTotal[] } {
  const summed = entities.map((e) => ({
    ...e,
    up: e.points.reduce((s, p) => s + p.up, 0),
    down: e.points.reduce((s, p) => s + p.down, 0),
  }));
  const limit = summed.length > SERIES_COLORS.length ? SERIES_COLORS.length - 1 : summed.length;
  const shown = [...summed]
    .sort((a, b) => b.up + b.down - (a.up + a.down))
    .slice(0, limit)
    .sort((a, b) => a.id - b.id);
  const color = new Map(shown.map((e, i) => [e.id, SERIES_COLORS[i] ?? OTHER_COLOR]));
  const hasOther = summed.length > shown.length;

  const days = eachDay(from, to);
  const index = new Map(days.map((d, i) => [d, i]));
  const rows: ChartRow[] = days.map((day) => {
    const row: ChartRow = { day };
    for (const e of shown) {
      row[`s${e.id}`] = 0;
    }
    if (hasOther) {
      row.other = 0;
    }
    return row;
  });
  for (const e of summed) {
    const key = color.has(e.id) ? `s${e.id}` : "other";
    for (const p of e.points) {
      const i = index.get(p.day);
      const row = i === undefined ? undefined : rows[i];
      if (row) {
        row[key] = Number(row[key] ?? 0) + p.up + p.down;
      }
    }
  }

  const series: ChartSeries[] = shown.map((e) => ({
    key: `s${e.id}`,
    name: e.name,
    color: color.get(e.id) ?? OTHER_COLOR,
  }));
  if (hasOther) {
    series.push({
      key: "other",
      name: `其他（${summed.length - shown.length} 个）`,
      color: OTHER_COLOR,
    });
  }
  const totals: EntityTotal[] = [...summed]
    .sort((a, b) => b.up + b.down - (a.up + a.down))
    .map((e) => ({
      id: e.id,
      name: e.name,
      up: e.up,
      down: e.down,
      color: color.get(e.id) ?? OTHER_COLOR,
    }));
  return { rows, series, totals };
}

/** 图例里的小色块（表格里标出对应的序列）。 */
export function Swatch({ color }: { color: string }): ReactNode {
  return (
    <span className="inline-block size-2.5 shrink-0 rounded-sm" style={{ background: color }} />
  );
}

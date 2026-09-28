import { Button, Input } from "@heroui/react";
import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { settingsQuery } from "../api/queries";
import { addDays, daysBetween, isDate, monthStart, previousMonth, todayIn } from "../lib/date";

export interface DateRangeValue {
  from: string;
  to: string;
}

type PresetId = "7d" | "30d" | "90d" | "month" | "last-month";

const PRESETS: { id: PresetId; label: string }[] = [
  { id: "7d", label: "最近 7 天" },
  { id: "30d", label: "最近 30 天" },
  { id: "90d", label: "最近 90 天" },
  { id: "month", label: "本月" },
  { id: "last-month", label: "上月" },
];

/** 主控限制：日期范围最长一年（stats.rs） */
const MAX_DAYS = 366;

export function presetRange(id: PresetId, today: string): DateRangeValue {
  switch (id) {
    case "7d":
      return { from: addDays(today, -6), to: today };
    case "90d":
      return { from: addDays(today, -89), to: today };
    case "month":
      return { from: monthStart(today), to: today };
    case "last-month":
      return previousMonth(today);
    default:
      return { from: addDays(today, -29), to: today };
  }
}

/** 管理员时区的今天（日账本按它分日）；设置还没加载时用浏览器时区。 */
export function useToday(): string {
  const settings = useQuery(settingsQuery);
  return todayIn(settings.data?.timezone);
}

/** 日期范围：默认最近 30 天，由调用方保存状态。 */
export function useDateRange(): [DateRangeValue, (value: DateRangeValue) => void, string] {
  const today = useToday();
  const [range, setRange] = useState<DateRangeValue | null>(null);
  return [range ?? presetRange("30d", today), setRange, today];
}

interface DateRangeBarProps {
  value: DateRangeValue;
  onChange: (value: DateRangeValue) => void;
  today: string;
}

/** 日期范围选择：常用范围一键选，也可以自己填起止日期。 */
export function DateRangeBar({ value, onChange, today }: DateRangeBarProps) {
  const [error, setError] = useState("");
  const active = PRESETS.find((p) => {
    const r = presetRange(p.id, today);
    return r.from === value.from && r.to === value.to;
  })?.id;

  const change = (next: DateRangeValue) => {
    if (!isDate(next.from) || !isDate(next.to)) {
      setError("日期格式不对");
      return;
    }
    const days = daysBetween(next.from, next.to);
    if (days < 0) {
      setError("开始日期不能晚于结束日期");
      return;
    }
    if (days > MAX_DAYS) {
      setError("最长只能查一年");
      return;
    }
    setError("");
    onChange(next);
  };

  return (
    <div className="flex flex-wrap items-center gap-2">
      {PRESETS.map((preset) => (
        <Button
          key={preset.id}
          size="sm"
          variant={active === preset.id ? "secondary" : "ghost"}
          onPress={() => change(presetRange(preset.id, today))}
        >
          {preset.label}
        </Button>
      ))}
      <div className="flex items-center gap-1.5 text-sm text-muted">
        <Input
          aria-label="开始日期"
          type="date"
          className="w-40"
          value={value.from}
          max={today}
          onChange={(e) => change({ from: e.target.value, to: value.to })}
        />
        至
        <Input
          aria-label="结束日期"
          type="date"
          className="w-40"
          value={value.to}
          max={today}
          onChange={(e) => change({ from: value.from, to: e.target.value })}
        />
      </div>
      {error ? <span className="text-sm text-danger">{error}</span> : null}
    </div>
  );
}

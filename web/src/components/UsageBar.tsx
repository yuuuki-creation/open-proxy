import { Label, Meter } from "@heroui/react";
import { formatBytes, formatQuota, usagePercent } from "../lib/format";

interface UsageBarProps {
  used: number;
  /** null 表示不限 */
  quota: number | null | undefined;
  label?: string;
  className?: string;
}

/** 已用 / 额度的进度条；不限额度时只显示用量。80% 以上变黄，用完变红。 */
export function UsageBar({ used, quota, label = "用量", className }: UsageBarProps) {
  const percent = usagePercent(used, quota);
  if (percent === null) {
    return (
      <div className={`text-sm ${className ?? ""}`}>
        {formatBytes(used)}
        <span className="text-muted"> / 不限</span>
      </div>
    );
  }
  const color = percent >= 100 ? "danger" : percent >= 80 ? "warning" : "accent";
  return (
    <Meter
      aria-label={label}
      className={`w-full min-w-40 ${className ?? ""}`}
      value={percent}
      color={color}
      size="sm"
    >
      <Label className="text-xs font-normal text-foreground">
        {formatBytes(used)}
        <span className="text-muted"> / {formatQuota(quota)}</span>
      </Label>
      <Meter.Output className="text-xs" />
      <Meter.Track>
        <Meter.Fill />
      </Meter.Track>
    </Meter>
  );
}

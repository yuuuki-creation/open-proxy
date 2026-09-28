// 显示用的格式化：流量按 1024 进制换算（1 GB = 1024³ 字节），时间按浏览器本地时区显示。

const UNITS = ["B", "KB", "MB", "GB", "TB", "PB"];

export const GB = 1024 ** 3;

/** 字节数换成带单位的文字，例如 1536 → "1.50 KB"。 */
export function formatBytes(bytes: number | null | undefined, digits = 2): string {
  if (bytes === null || bytes === undefined || !Number.isFinite(bytes)) {
    return "-";
  }
  const sign = bytes < 0 ? "-" : "";
  let value = Math.abs(bytes);
  let unit = 0;
  while (value >= 1024 && unit < UNITS.length - 1) {
    value /= 1024;
    unit += 1;
  }
  const text = unit === 0 ? String(Math.round(value)) : value.toFixed(digits);
  return `${sign}${text} ${UNITS[unit]}`;
}

/** 网速（字节/秒）。 */
export function formatSpeed(bytesPerSecond: number): string {
  return `${formatBytes(bytesPerSecond, 1)}/s`;
}

/** 额度：null 表示不限。 */
export function formatQuota(bytes: number | null | undefined): string {
  if (bytes === null || bytes === undefined) {
    return "不限";
  }
  return formatGb(bytes);
}

/** 按 GB 显示，整数不带小数，例如 107374182400 → "100 GB"。 */
export function formatGb(bytes: number): string {
  const gb = bytes / GB;
  const text = Number.isInteger(gb) ? String(gb) : gb.toFixed(2).replace(/\.?0+$/, "");
  return `${text} GB`;
}

/** 字节数换成 GB 数（表单里编辑额度用），保留两位小数。 */
export function bytesToGbInput(bytes: number | null | undefined): string {
  if (bytes === null || bytes === undefined) {
    return "";
  }
  const gb = bytes / GB;
  return Number.isInteger(gb) ? String(gb) : gb.toFixed(2).replace(/\.?0+$/, "");
}

/** GB 数换成字节数。 */
export function gbToBytes(gb: number): number {
  return Math.round(gb * GB);
}

/** 用量占额度的百分比，0–100；不限额度时返回 null。 */
export function usagePercent(used: number, quota: number | null | undefined): number | null {
  if (quota === null || quota === undefined) {
    return null;
  }
  if (quota <= 0) {
    return used > 0 ? 100 : 0;
  }
  return Math.min(100, Math.max(0, (used / quota) * 100));
}

const pad = (n: number) => String(n).padStart(2, "0");

/** RFC 3339 时间显示成本地的 "2026-09-27 15:04"。 */
export function formatDateTime(iso: string | null | undefined): string {
  if (!iso) {
    return "-";
  }
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) {
    return iso;
  }
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

/** 相对时间："刚刚"、"3 分钟前"、"2 小时前"、"5 天前"。 */
export function formatRelative(iso: string | null | undefined, now = Date.now()): string {
  if (!iso) {
    return "从未";
  }
  const time = new Date(iso).getTime();
  if (Number.isNaN(time)) {
    return iso;
  }
  const seconds = Math.max(0, Math.round((now - time) / 1000));
  if (seconds < 60) {
    return "刚刚";
  }
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) {
    return `${minutes} 分钟前`;
  }
  const hours = Math.floor(minutes / 60);
  if (hours < 24) {
    return `${hours} 小时前`;
  }
  return `${Math.floor(hours / 24)} 天前`;
}

/** 数字加千分位。 */
export function formatNumber(n: number): string {
  return n.toLocaleString("zh-CN");
}

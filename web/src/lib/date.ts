// 日期（YYYY-MM-DD）的计算。开通日、到期日、日账本都按管理员设置的时区算（database.md），
// 所以「今天」要用管理员时区；日期之间的加减按 UTC 的日历日算，不受浏览器时区影响。

const DATE_RE = /^(\d{4})-(\d{2})-(\d{2})$/;

/** 是不是合法的 YYYY-MM-DD。 */
export function isDate(value: string): boolean {
  const m = DATE_RE.exec(value);
  if (!m) {
    return false;
  }
  const date = new Date(Date.UTC(Number(m[1]), Number(m[2]) - 1, Number(m[3])));
  return toDate(date) === value;
}

function parse(value: string): Date {
  const m = DATE_RE.exec(value);
  if (!m) {
    return new Date(Number.NaN);
  }
  return new Date(Date.UTC(Number(m[1]), Number(m[2]) - 1, Number(m[3])));
}

function toDate(date: Date): string {
  const y = date.getUTCFullYear();
  const m = String(date.getUTCMonth() + 1).padStart(2, "0");
  const d = String(date.getUTCDate()).padStart(2, "0");
  return `${y}-${m}-${d}`;
}

/** 某个时区（IANA 名字）的今天；时区不认识时用浏览器的时区。 */
export function todayIn(timeZone?: string): string {
  const options: Intl.DateTimeFormatOptions = {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
  };
  try {
    // en-CA 的日期格式正好是 YYYY-MM-DD
    return new Intl.DateTimeFormat("en-CA", { ...options, timeZone }).format(new Date());
  } catch {
    return new Intl.DateTimeFormat("en-CA", options).format(new Date());
  }
}

/** 浏览器的时区，例如 "Asia/Shanghai"。 */
export function browserTimeZone(): string {
  try {
    return Intl.DateTimeFormat().resolvedOptions().timeZone || "Asia/Shanghai";
  } catch {
    return "Asia/Shanghai";
  }
}

/** 常用时区列表（浏览器支持时用完整列表）。 */
export function timeZones(): string[] {
  let list: string[] = [];
  try {
    list = Intl.supportedValuesOf("timeZone");
  } catch {
    list = [];
  }
  const common = ["Asia/Shanghai", "Asia/Hong_Kong", "Asia/Taipei", "Asia/Tokyo", "UTC"];
  return Array.from(new Set([...common, ...list]));
}

export function addDays(value: string, days: number): string {
  const date = parse(value);
  date.setUTCDate(date.getUTCDate() + days);
  return toDate(date);
}

/** 加几个月；目标月份没有这一天时夹到月末（例如 1 月 31 日加一个月是 2 月 28 日）。 */
export function addMonths(value: string, months: number): string {
  const date = parse(value);
  const day = date.getUTCDate();
  const target = new Date(Date.UTC(date.getUTCFullYear(), date.getUTCMonth() + months, 1));
  const lastDay = new Date(
    Date.UTC(target.getUTCFullYear(), target.getUTCMonth() + 1, 0),
  ).getUTCDate();
  target.setUTCDate(Math.min(day, lastDay));
  return toDate(target);
}

/** 两个日期相差几天（b - a）。 */
export function daysBetween(a: string, b: string): number {
  return Math.round((parse(b).getTime() - parse(a).getTime()) / 86_400_000);
}

/** 这个月的 1 号。 */
export function monthStart(value: string): string {
  return `${value.slice(0, 7)}-01`;
}

/** 上个月的第一天和最后一天。 */
export function previousMonth(value: string): { from: string; to: string } {
  const first = parse(monthStart(value));
  first.setUTCMonth(first.getUTCMonth() - 1);
  const from = toDate(first);
  const to = addDays(monthStart(value), -1);
  return { from, to };
}

/** from 到 to 之间的每一天（含两端）。 */
export function eachDay(from: string, to: string): string[] {
  const days: string[] = [];
  const total = daysBetween(from, to);
  if (!Number.isFinite(total) || total < 0 || total > 400) {
    return days;
  }
  for (let i = 0; i <= total; i += 1) {
    days.push(addDays(from, i));
  }
  return days;
}

/** 图表横轴上显示的短日期："09-27"。 */
export function shortDay(value: string): string {
  return value.slice(5);
}

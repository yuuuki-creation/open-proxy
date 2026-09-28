import type { ReactNode } from "react";

export interface InfoItem {
  /** 同一个列表里不能重复 */
  label: string;
  value: ReactNode;
  /** 占满一整行（长内容） */
  wide?: boolean;
}

/** 详情页的「名称：值」列表，两列排布。 */
export function InfoList({ items }: { items: InfoItem[] }) {
  return (
    <dl className="grid grid-cols-1 gap-x-8 gap-y-3 text-sm md:grid-cols-2">
      {items.map((item) => (
        <div key={item.label} className={`flex min-w-0 gap-3 ${item.wide ? "md:col-span-2" : ""}`}>
          <dt className="w-28 shrink-0 text-muted">{item.label}</dt>
          <dd className="min-w-0 flex-1 break-words text-foreground">{item.value}</dd>
        </div>
      ))}
    </dl>
  );
}

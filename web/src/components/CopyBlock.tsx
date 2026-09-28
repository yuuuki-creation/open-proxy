import { Copy } from "@gravity-ui/icons";
import { Button } from "@heroui/react";
import type { ReactNode } from "react";
import { copyWithToast } from "../lib/clipboard";

interface CopyBlockProps {
  value: string;
  label?: ReactNode;
  /** 复制成功提示里的名字，例如「安装命令」 */
  what?: string;
}

/** 一段可以一键复制的文字（安装命令、订阅链接等），等宽显示，点一下就能全选。 */
export function CopyBlock({ value, label, what }: CopyBlockProps) {
  return (
    <div className="flex flex-col gap-1.5">
      {label ? <span className="text-sm font-medium text-foreground">{label}</span> : null}
      <div className="flex items-start gap-2 rounded-xl bg-surface-secondary p-3">
        <code className="flex-1 select-all break-all font-mono text-xs leading-relaxed text-foreground">
          {value}
        </code>
        <Button size="sm" variant="secondary" onPress={() => void copyWithToast(value, what)}>
          <Copy />
          复制
        </Button>
      </div>
    </div>
  );
}

/** 表格里的小复制按钮。 */
export function CopyIconButton({
  value,
  what,
  label,
}: {
  value: string;
  what?: string;
  label: string;
}) {
  return (
    <Button
      isIconOnly
      size="sm"
      variant="ghost"
      aria-label={label}
      onPress={() => void copyWithToast(value, what)}
    >
      <Copy className="size-4" />
    </Button>
  );
}

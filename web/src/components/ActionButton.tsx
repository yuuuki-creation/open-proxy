import { Button, type ButtonProps, Spinner } from "@heroui/react";
import type { ReactNode } from "react";

type ActionButtonProps = Omit<ButtonProps, "children"> & { children: ReactNode };

/** 带加载状态的按钮：isPending 时显示转圈，并且不能重复点击。 */
export function ActionButton({ isPending, children, ...rest }: ActionButtonProps) {
  return (
    <Button isPending={isPending} {...rest}>
      {isPending ? <Spinner color="current" size="sm" /> : null}
      {children}
    </Button>
  );
}

import { AlertDialog, Button } from "@heroui/react";
import { type ReactNode, useState } from "react";
import { ActionButton } from "./ActionButton";

interface ConfirmDialogProps {
  isOpen: boolean;
  onOpenChange: (isOpen: boolean) => void;
  title: ReactNode;
  children: ReactNode;
  confirmLabel?: string;
  status?: "danger" | "warning" | "accent";
  /** 返回 Promise 时按钮显示加载中，成功后关闭；失败时保持打开（错误由全局提示显示） */
  onConfirm: () => unknown;
}

/** 确认框：删除、重置等不能撤销的操作先问一句。 */
export function ConfirmDialog({
  isOpen,
  onOpenChange,
  title,
  children,
  confirmLabel = "确定",
  status = "danger",
  onConfirm,
}: ConfirmDialogProps) {
  const [pending, setPending] = useState(false);

  const confirm = async () => {
    setPending(true);
    try {
      await onConfirm();
      onOpenChange(false);
    } catch {
      // 错误已经由请求的全局错误提示显示
    } finally {
      setPending(false);
    }
  };

  return (
    <AlertDialog.Backdrop
      isOpen={isOpen}
      onOpenChange={(open) => {
        if (!pending) {
          onOpenChange(open);
        }
      }}
    >
      <AlertDialog.Container>
        <AlertDialog.Dialog className="sm:max-w-[460px]">
          <AlertDialog.Header>
            <AlertDialog.Icon status={status} />
            <AlertDialog.Heading>{title}</AlertDialog.Heading>
          </AlertDialog.Header>
          <AlertDialog.Body>
            <div className="flex flex-col gap-2 text-sm">{children}</div>
          </AlertDialog.Body>
          <AlertDialog.Footer>
            <Button slot="close" variant="tertiary" isDisabled={pending}>
              取消
            </Button>
            <ActionButton
              variant={status === "danger" ? "danger" : "primary"}
              isPending={pending}
              onPress={confirm}
            >
              {confirmLabel}
            </ActionButton>
          </AlertDialog.Footer>
        </AlertDialog.Dialog>
      </AlertDialog.Container>
    </AlertDialog.Backdrop>
  );
}

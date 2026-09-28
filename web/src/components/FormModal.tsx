import { Button, Form, Modal } from "@heroui/react";
import { type FormEvent, type ReactNode, useId } from "react";
import { ActionButton } from "./ActionButton";

interface FormModalProps {
  /** 关闭弹窗。弹窗只在需要时挂载（打开时才渲染），表单的初始值在挂载时确定 */
  onClose: () => void;
  title: ReactNode;
  description?: ReactNode;
  size?: "sm" | "md" | "lg";
  submitLabel?: string;
  isSubmitting?: boolean;
  /** 一般传 react-hook-form 的 handleSubmit(...) */
  onSubmit: (event: FormEvent<HTMLFormElement>) => unknown;
  children: ReactNode;
  /** 提交按钮不可用（例如还没检测通过） */
  isSubmitDisabled?: boolean;
  /** 放在底部按钮左边的内容 */
  footerStart?: ReactNode;
}

/** 弹窗表单：标题 + 表单 + 底部「取消 / 保存」。提交中不能关闭。 */
export function FormModal({
  onClose,
  title,
  description,
  size = "md",
  submitLabel = "保存",
  isSubmitting = false,
  onSubmit,
  children,
  isSubmitDisabled = false,
  footerStart,
}: FormModalProps) {
  const formId = useId();
  return (
    <Modal.Backdrop
      isOpen
      onOpenChange={(open) => {
        if (!open && !isSubmitting) {
          onClose();
        }
      }}
      isDismissable={!isSubmitting}
    >
      <Modal.Container size={size} scroll="inside">
        <Modal.Dialog>
          <Modal.CloseTrigger />
          <Modal.Header>
            <Modal.Heading>{title}</Modal.Heading>
            {description ? (
              <p className="mt-1 text-sm leading-5 text-muted">{description}</p>
            ) : null}
          </Modal.Header>
          <Modal.Body>
            <Form
              id={formId}
              className="flex flex-col gap-4 p-0.5"
              validationBehavior="aria"
              onSubmit={(event) => {
                // 提交失败时主控的错误信息已经由全局提示显示，弹窗保持打开
                Promise.resolve(onSubmit(event)).catch(() => {});
              }}
            >
              {children}
            </Form>
          </Modal.Body>
          <Modal.Footer>
            {footerStart ? <div className="mr-auto">{footerStart}</div> : null}
            <Button slot="close" variant="tertiary" isDisabled={isSubmitting}>
              取消
            </Button>
            <ActionButton
              type="submit"
              form={formId}
              isPending={isSubmitting}
              isDisabled={isSubmitDisabled}
            >
              {submitLabel}
            </ActionButton>
          </Modal.Footer>
        </Modal.Dialog>
      </Modal.Container>
    </Modal.Backdrop>
  );
}

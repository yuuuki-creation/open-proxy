import { toast } from "@heroui/react";

/** 复制文字。剪贴板接口只在 HTTPS 或本机地址下可用，不可用时退回旧的 execCommand。 */
export async function copyText(text: string): Promise<boolean> {
  if (navigator.clipboard && window.isSecureContext) {
    try {
      await navigator.clipboard.writeText(text);
      return true;
    } catch {
      // 权限被拒时走下面的办法
    }
  }
  const area = document.createElement("textarea");
  area.value = text;
  area.setAttribute("readonly", "");
  area.style.position = "fixed";
  area.style.opacity = "0";
  document.body.append(area);
  area.select();
  let ok = false;
  try {
    ok = document.execCommand("copy");
  } catch {
    ok = false;
  }
  area.remove();
  return ok;
}

/** 复制并提示结果。 */
export async function copyWithToast(text: string, what = "内容"): Promise<void> {
  if (await copyText(text)) {
    toast.success(`已复制${what}`);
  } else {
    toast.danger("复制失败，请手动选中复制");
  }
}

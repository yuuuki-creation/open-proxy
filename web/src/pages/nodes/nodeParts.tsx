// 节点创建向导和编辑弹窗共用的部分：表单校验、伪装目标（检测通过才能保存）、Hysteria2 的混淆和端口跳跃、出口选项。

import { CircleCheck, TriangleExclamation } from "@gravity-ui/icons";
import {
  Button,
  Chip,
  Description,
  Disclosure,
  FieldError,
  Input,
  Label,
  TextField,
} from "@heroui/react";
import { useMutation } from "@tanstack/react-query";
import { type Control, useController, useWatch } from "react-hook-form";
import { z } from "zod";
import { serversApi } from "../../api/endpoints";
import type { Exit, Protocol, SubFormat } from "../../api/types";
import { ActionButton } from "../../components/ActionButton";
import { type Option, SwitchInput, TextInput } from "../../components/form";
import { checkPassed, normalizeTarget, RealityTools } from "../../components/reality";
import { FORMAT_LABELS } from "../../lib/labels";
import { nameSchema, optionalAddressSchema, optionalPortSchema } from "../../lib/validation";

export const nodeSchema = z
  .object({
    server_id: z.string().min(1, "请选择服务器"),
    protocol: z.string().min(1, "请选择协议"),
    name: nameSchema("节点名"),
    port: optionalPortSchema,
    address: optionalAddressSchema,
    reality_target: z.string().trim(),
    obfs: z.boolean(),
    hop_enabled: z.boolean(),
    hop_start: z.string().trim(),
    hop_end: z.string().trim(),
    exit_id: z.string(),
    enabled: z.boolean(),
  })
  .superRefine((v, ctx) => {
    if (v.protocol === "vless_reality" && normalizeTarget(v.reality_target) === null) {
      ctx.addIssue({
        code: "custom",
        message: "伪装目标要写成 域名 或 域名:端口，例如 www.example.com:443",
        path: ["reality_target"],
      });
    }
    if (v.protocol === "hysteria2" && v.hop_enabled) {
      const isPort = (s: string) => /^\d+$/.test(s) && Number(s) >= 1 && Number(s) <= 65535;
      if (!isPort(v.hop_start)) {
        ctx.addIssue({ code: "custom", message: "端口要在 1–65535 之间", path: ["hop_start"] });
      }
      if (!isPort(v.hop_end)) {
        ctx.addIssue({ code: "custom", message: "端口要在 1–65535 之间", path: ["hop_end"] });
      } else if (isPort(v.hop_start) && Number(v.hop_start) >= Number(v.hop_end)) {
        ctx.addIssue({ code: "custom", message: "终点要大于起点", path: ["hop_end"] });
      }
      if (
        isPort(v.hop_start) &&
        isPort(v.hop_end) &&
        v.port &&
        Number(v.port) >= Number(v.hop_start) &&
        Number(v.port) <= Number(v.hop_end)
      ) {
        ctx.addIssue({
          code: "custom",
          message: "端口跳跃范围不能包含节点自己的端口",
          path: ["hop_start"],
        });
      }
    }
  });

export type NodeFormValues = z.infer<typeof nodeSchema>;

/** 表单里「直连」的选项 id（下拉框的空值会显示成「请选择」，所以用一个专门的值） */
export const DIRECT = "direct";

/** 表单里的出口选项转成接口的 exit_id：直连是 null。 */
export function toExitId(value: string): number | null {
  return value && value !== DIRECT ? Number(value) : null;
}

/** 出口下拉选项：直连 + 已登记的出口。 */
export function exitOptions(exits: Exit[]): Option[] {
  return [
    { id: DIRECT, label: "直连（不走落地出口）" },
    ...exits.map((e) => ({
      id: String(e.id),
      label: `${e.name}（${e.kind === "self_built" ? "自建" : "第三方"}，${e.host}:${e.port}）`,
    })),
  ];
}

/** 「在哪些客户端里看不到」的标签。 */
export function HiddenIn({ formats }: { formats: SubFormat[] | undefined }) {
  if (formats === undefined) {
    return <span className="text-xs text-muted">-</span>;
  }
  if (formats.length === 0) {
    return <span className="text-xs text-muted">全部可见</span>;
  }
  return (
    <div className="flex flex-wrap gap-1">
      {formats.map((f) => (
        <Chip key={f} size="sm" color="warning" variant="soft">
          {FORMAT_LABELS[f] ?? f}
        </Chip>
      ))}
    </div>
  );
}

interface RealityTargetFieldProps {
  control: Control<NodeFormValues>;
  serverId: number | null;
  /** 检测通过的目标（统一写法），和当前填的一致才能保存 */
  checked: string | null;
  onChecked: (target: string | null) => void;
  /** 编辑时原来的目标：没改就不用重新检测 */
  original?: string;
}

/** VLESS + REALITY 的伪装目标：填写后要由 Agent 检测通过（TLS 1.3、H2、证书有效）才能保存。 */
export function RealityTargetField({
  control,
  serverId,
  checked,
  onChecked,
  original,
}: RealityTargetFieldProps) {
  const { field, fieldState } = useController({ control, name: "reality_target" });
  const normalized = normalizeTarget((field.value as string) ?? "");
  const unchanged = original !== undefined && normalized === normalizeTarget(original);
  const check = useMutation({
    mutationFn: (target: string) => serversApi.realityCheck(serverId ?? 0, [target]),
    meta: { keepCache: true },
    onSuccess: (data, target) => {
      const result = data.results[0];
      onChecked(result && checkPassed(result) ? target : null);
    },
  });
  const result = check.data?.results[0];
  const passed = unchanged || (normalized !== null && checked === normalized);

  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-start gap-2">
        <TextField
          className="flex-1"
          fullWidth
          name={field.name}
          value={(field.value as string) ?? ""}
          onChange={field.onChange}
          onBlur={field.onBlur}
          isInvalid={fieldState.invalid}
          isRequired
          validationBehavior="aria"
        >
          <Label>伪装目标</Label>
          <Input ref={field.ref} className="font-mono" placeholder="www.example.com:443" />
          {fieldState.error ? null : (
            <Description>支持 TLS 1.3 和 H2、离服务器近的网站；不写端口就是 443。</Description>
          )}
          <FieldError>{fieldState.error?.message}</FieldError>
        </TextField>
        <ActionButton
          className="mt-6"
          variant="secondary"
          isPending={check.isPending}
          isDisabled={!normalized || serverId === null}
          onPress={() => {
            if (normalized) {
              check.mutate(normalized);
            }
          }}
        >
          检测
        </ActionButton>
      </div>
      {check.isPending ? (
        <span className="text-xs text-muted">Agent 正在服务器上访问这个网站，最多等 1 分钟…</span>
      ) : passed ? (
        <span className="inline-flex items-center gap-1 text-xs text-success">
          <CircleCheck className="size-3.5" />
          {unchanged ? "伪装目标没有改动" : "检测通过"}
          {result && !unchanged && checkPassed(result) ? `，延迟 ${result.latency_ms} ms` : ""}
        </span>
      ) : result && normalized === normalizeTarget(result.target) ? (
        <span className="inline-flex items-center gap-1 text-xs text-danger">
          <TriangleExclamation className="size-3.5" />
          检测没通过：
          {result.error ||
            [
              result.tls13 ? null : "不支持 TLS 1.3",
              result.h2 ? null : "不支持 H2",
              result.certificate_valid ? null : "证书无效",
            ]
              .filter(Boolean)
              .join("、")}
        </span>
      ) : (
        <span className="text-xs text-warning">伪装目标要先检测通过才能保存。</span>
      )}
      {serverId !== null ? (
        <Disclosure>
          <Disclosure.Heading>
            <Button slot="trigger" size="sm" variant="ghost">
              找候选目标（批量检测 / 让 Agent 扫描）
              <Disclosure.Indicator />
            </Button>
          </Disclosure.Heading>
          <Disclosure.Content>
            <Disclosure.Body className="mt-2 rounded-xl bg-surface-secondary p-4">
              <RealityTools
                serverId={serverId}
                onPick={(target) => {
                  field.onChange(target);
                  onChecked(normalizeTarget(target));
                }}
              />
            </Disclosure.Body>
          </Disclosure.Content>
        </Disclosure>
      ) : null}
    </div>
  );
}

/** Hysteria2：salamander 混淆和端口跳跃（都是开关，默认关）。 */
export function HysteriaFields({ control }: { control: Control<NodeFormValues> }) {
  const hopEnabled = useWatch({ control, name: "hop_enabled" });
  return (
    <div className="flex flex-col gap-4 rounded-xl bg-surface-secondary p-4">
      <SwitchInput
        control={control}
        name="obfs"
        label="salamander 混淆"
        description="开了以后 Surge 看不到这个节点（Quantumult X 本来就不支持 Hysteria2）。混淆密码由主控生成。"
      />
      <SwitchInput
        control={control}
        name="hop_enabled"
        label="端口跳跃"
        description="把一段 UDP 端口都转发到节点端口，客户端在这些端口之间跳着连，抗 QoS。"
      />
      {hopEnabled ? (
        <div className="grid grid-cols-2 gap-4">
          <TextInput
            control={control}
            name="hop_start"
            label="跳跃范围起点"
            inputMode="numeric"
            isRequired
            placeholder="例如 30000"
          />
          <TextInput
            control={control}
            name="hop_end"
            label="跳跃范围终点"
            inputMode="numeric"
            isRequired
            placeholder="例如 30999"
          />
        </div>
      ) : null}
    </div>
  );
}

/** 表单值里的协议（空字符串表示还没选）。 */
export function asProtocol(value: string): Protocol | null {
  const all: Protocol[] = ["vless_reality", "hysteria2", "anytls", "shadowsocks2022", "mieru"];
  return all.includes(value as Protocol) ? (value as Protocol) : null;
}

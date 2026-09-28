import { zodResolver } from "@hookform/resolvers/zod";
import { useMutation } from "@tanstack/react-query";
import { useForm, useWatch } from "react-hook-form";
import { z } from "zod";
import { serversApi } from "../../api/endpoints";
import type { CreateServerResponse, Server, ServerFields } from "../../api/types";
import { FormModal } from "../../components/FormModal";
import { RadioInput, TextInput } from "../../components/form";
import { bytesToGbInput, gbToBytes } from "../../lib/format";
import {
  addressSchema,
  isHostname,
  nameSchema,
  optionalGbSchema,
  optionalIntSchema,
  portSchema,
  toNumber,
} from "../../lib/validation";

const schema = z
  .object({
    name: nameSchema("服务器名"),
    address: addressSchema,
    port_range_start: portSchema,
    port_range_end: portSchema,
    cert_mode: z.enum(["self_signed", "acme"]),
    cert_domain: z.string().trim().toLowerCase(),
    quota_gb: optionalGbSchema,
    reset_day: optionalIntSchema(1, 31, "重置日要在 1–31 之间"),
  })
  .superRefine((v, ctx) => {
    if (Number(v.port_range_start) > Number(v.port_range_end)) {
      ctx.addIssue({
        code: "custom",
        message: "起点不能大于终点",
        path: ["port_range_end"],
      });
    }
    if (v.cert_mode === "acme" && !isHostname(v.cert_domain)) {
      ctx.addIssue({
        code: "custom",
        message: "自动申请证书要填这台服务器的域名",
        path: ["cert_domain"],
      });
    }
  });

type Values = z.infer<typeof schema>;

function toValues(server: Server | null): Values {
  return {
    name: server?.name ?? "",
    address: server?.address ?? "",
    port_range_start: String(server?.port_range_start ?? 10000),
    port_range_end: String(server?.port_range_end ?? 60000),
    cert_mode: server?.cert_mode ?? "self_signed",
    cert_domain: server?.cert_domain ?? "",
    quota_gb: bytesToGbInput(server?.traffic_quota_bytes),
    reset_day: server?.traffic_reset_day ? String(server.traffic_reset_day) : "",
  };
}

function toFields(v: Values): ServerFields {
  const quota = toNumber(v.quota_gb);
  const resetDay = toNumber(v.reset_day);
  return {
    name: v.name,
    address: v.address,
    port_range_start: Number(v.port_range_start),
    port_range_end: Number(v.port_range_end),
    cert_mode: v.cert_mode,
    cert_domain: v.cert_mode === "acme" ? v.cert_domain : null,
    traffic_quota_bytes: quota === undefined ? null : gbToBytes(quota),
    traffic_reset_day: resetDay ?? null,
  };
}

interface ServerFormModalProps {
  onClose: () => void;
  /** 为 null 时是添加服务器 */
  server: Server | null;
  /** 添加成功后拿到安装命令（Token 只出现这一次） */
  onCreated?: (result: CreateServerResponse) => void;
}

/** 添加或编辑服务器。打开时才挂载，表单初始值取打开那一刻的服务器信息。 */
export function ServerFormModal({ onClose, server, onCreated }: ServerFormModalProps) {
  const form = useForm<Values>({ resolver: zodResolver(schema), defaultValues: toValues(server) });
  const certMode = useWatch({ control: form.control, name: "cert_mode" });

  const create = useMutation({ mutationFn: serversApi.create });
  const update = useMutation({
    mutationFn: (fields: ServerFields) => serversApi.update(server?.id ?? 0, fields),
  });

  const submit = form.handleSubmit(async (v) => {
    const fields = toFields(v);
    if (server) {
      await update.mutateAsync(fields);
    } else {
      const result = await create.mutateAsync(fields);
      onCreated?.(result);
    }
    onClose();
  });

  return (
    <FormModal
      onClose={onClose}
      title={server ? `编辑服务器「${server.name}」` : "添加服务器"}
      description={
        server
          ? "改证书方式或域名后，自签证书立即重新生成；自动申请的证书由主控重新申请。"
          : "添加后会给出安装命令，到这台服务器上执行就会装好 Agent 并连上主控。"
      }
      size="lg"
      submitLabel={server ? "保存" : "添加"}
      isSubmitting={create.isPending || update.isPending}
      onSubmit={submit}
    >
      <div className="grid grid-cols-2 gap-4">
        <TextInput
          control={form.control}
          name="name"
          label="名称"
          isRequired
          placeholder="例如 东京-1"
        />
        <TextInput
          control={form.control}
          name="address"
          label="地址"
          isRequired
          placeholder="IPv4 或域名"
          description="节点默认用这个地址；只支持 IPv4。"
        />
        <TextInput
          control={form.control}
          name="port_range_start"
          label="节点端口范围起点"
          inputMode="numeric"
          description="新建节点时在这个范围里随机分配端口。"
        />
        <TextInput
          control={form.control}
          name="port_range_end"
          label="节点端口范围终点"
          inputMode="numeric"
        />
      </div>
      <RadioInput
        control={form.control}
        name="cert_mode"
        label="证书（Hysteria2、AnyTLS 节点用）"
        orientation="horizontal"
        options={[
          {
            id: "self_signed",
            label: "自签证书",
            description: "没有域名时用，订阅里固定证书指纹。",
          },
          {
            id: "acme",
            label: "自动申请",
            description: "服务器要有域名，主控用 Cloudflare DNS 验证申请和续期。",
          },
        ]}
      />
      {certMode === "acme" ? (
        <TextInput
          control={form.control}
          name="cert_domain"
          label="这台服务器的域名"
          isRequired
          placeholder="node1.example.com"
          description="要在 Cloudflare 上，并且已经在设置里填了 Cloudflare Token。"
        />
      ) : null}
      <div className="grid grid-cols-2 gap-4">
        <TextInput
          control={form.control}
          name="quota_gb"
          label="整机月流量额度（GB）"
          inputMode="decimal"
          placeholder="不填表示不限"
          description="只用来显示进度，对应 VPS 服务商的流量额度。"
        />
        <TextInput
          control={form.control}
          name="reset_day"
          label="每月重置日"
          inputMode="numeric"
          placeholder="1–31，不填按每月 1 号"
          description="对应 VPS 的账单日；短月按月末算。"
        />
      </div>
    </FormModal>
  );
}

import { toast } from "@heroui/react";
import { zodResolver } from "@hookform/resolvers/zod";
import { useMutation } from "@tanstack/react-query";
import { useState } from "react";
import { useForm, useWatch } from "react-hook-form";
import { nodesApi } from "../../api/endpoints";
import type { Exit, Node, UpdateNode } from "../../api/types";
import { CopyIconButton } from "../../components/CopyBlock";
import { FormModal } from "../../components/FormModal";
import { SelectInput, SwitchInput, TextInput } from "../../components/form";
import { InfoList } from "../../components/InfoList";
import { normalizeTarget } from "../../components/reality";
import { FORMAT_LABELS, predictHiddenIn, protocolLabel } from "../../lib/labels";
import {
  DIRECT,
  exitOptions,
  HysteriaFields,
  type NodeFormValues,
  nodeSchema,
  RealityTargetField,
  toExitId,
} from "./nodeParts";

function toValues(node: Node): NodeFormValues {
  return {
    server_id: String(node.server_id),
    protocol: node.protocol,
    name: node.name,
    port: String(node.port),
    address: node.address_override ?? "",
    reality_target: node.reality?.target ?? "",
    obfs: node.obfs,
    hop_enabled: node.hop_ports !== null,
    hop_start: node.hop_ports ? String(node.hop_ports.start) : "",
    hop_end: node.hop_ports ? String(node.hop_ports.end) : "",
    exit_id: node.exit_id === null ? DIRECT : String(node.exit_id),
    enabled: node.enabled,
  };
}

interface NodeEditModalProps {
  node: Node;
  exits: Exit[];
  onClose: () => void;
}

/** 编辑节点：服务器和协议不能改（要换就新建一个节点）。 */
export function NodeEditModal({ node, exits, onClose }: NodeEditModalProps) {
  const [checkedTarget, setCheckedTarget] = useState<string | null>(null);
  const form = useForm<NodeFormValues>({
    resolver: zodResolver(nodeSchema),
    defaultValues: toValues(node),
  });
  const [target, obfs] = useWatch({ control: form.control, name: ["reality_target", "obfs"] });
  const originalTarget = node.reality?.target;
  const targetChanged =
    node.protocol === "vless_reality" &&
    normalizeTarget(target) !== normalizeTarget(originalTarget ?? "");
  const targetReady =
    !targetChanged ||
    (normalizeTarget(target) !== null && checkedTarget === normalizeTarget(target));

  const update = useMutation({
    mutationFn: (body: UpdateNode) => nodesApi.update(node.id, body),
    onSuccess: (saved) => {
      toast.success(`已保存节点「${saved.name}」`);
      onClose();
    },
  });

  const submit = form.handleSubmit(async (v) => {
    const body: UpdateNode = {
      name: v.name,
      address: v.address ? v.address : null,
      exit_id: toExitId(v.exit_id),
      enabled: v.enabled,
    };
    if (v.port) {
      body.port = Number(v.port);
    }
    if (node.protocol === "vless_reality" && targetChanged) {
      body.reality_target = normalizeTarget(v.reality_target) ?? v.reality_target;
    }
    if (node.protocol === "hysteria2") {
      body.obfs = v.obfs;
      body.hop_ports = v.hop_enabled
        ? { start: Number(v.hop_start), end: Number(v.hop_end) }
        : null;
    }
    await update.mutateAsync(body);
  });

  const hidden = predictHiddenIn(node.protocol, obfs);

  return (
    <FormModal
      onClose={onClose}
      title={`编辑节点「${node.name}」`}
      description={`${node.server_name} 上的 ${protocolLabel(node.protocol)} 节点。服务器和协议不能改，要换就新建一个节点。`}
      size="lg"
      isSubmitting={update.isPending}
      isSubmitDisabled={!targetReady}
      onSubmit={submit}
    >
      <TextInput control={form.control} name="name" label="节点名" isRequired />
      <div className="grid grid-cols-2 gap-4">
        <TextInput
          control={form.control}
          name="port"
          label="端口"
          inputMode="numeric"
          description="改端口后客户端要更新订阅。"
        />
        <TextInput
          control={form.control}
          name="address"
          label="地址"
          placeholder="默认用服务器的地址"
          description="只有节点要用别的地址时才填；清空就改回服务器的地址。"
        />
      </div>
      {node.protocol === "vless_reality" ? (
        <>
          <RealityTargetField
            control={form.control}
            serverId={node.server_id}
            checked={checkedTarget}
            onChecked={setCheckedTarget}
            original={originalTarget}
          />
          {node.reality ? (
            <InfoList
              items={[
                {
                  label: "公钥",
                  value: (
                    <span className="inline-flex items-center gap-1 font-mono text-xs">
                      {node.reality.public_key}
                      <CopyIconButton
                        value={node.reality.public_key}
                        what="公钥"
                        label="复制公钥"
                      />
                    </span>
                  ),
                  wide: true,
                },
                {
                  label: "Short ID",
                  value: <span className="font-mono text-xs">{node.reality.short_id}</span>,
                },
              ]}
            />
          ) : null}
        </>
      ) : null}
      {node.protocol === "hysteria2" ? <HysteriaFields control={form.control} /> : null}
      <SelectInput
        control={form.control}
        name="exit_id"
        label="落地出口"
        options={exitOptions(exits)}
      />
      <SwitchInput
        control={form.control}
        name="enabled"
        label="启用"
        description="停用的节点不下发到服务器，也不进订阅。"
      />
      {hidden.length > 0 ? (
        <p className="text-xs text-warning">
          这些客户端里看不到这个节点：{hidden.map((f) => FORMAT_LABELS[f]).join("、")}。
        </p>
      ) : null}
    </FormModal>
  );
}

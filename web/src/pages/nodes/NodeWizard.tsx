import { Button, Form, Modal, toast } from "@heroui/react";
import { zodResolver } from "@hookform/resolvers/zod";
import { useMutation } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { useId, useState } from "react";
import { useForm, useWatch } from "react-hook-form";
import { nodesApi } from "../../api/endpoints";
import type { CreateNode, Exit, Server } from "../../api/types";
import { ActionButton } from "../../components/ActionButton";
import { RadioInput, SelectInput, SwitchInput, TextInput } from "../../components/form";
import { normalizeTarget } from "../../components/reality";
import { FORMAT_LABELS, PROTOCOLS, predictHiddenIn, protocolLabel } from "../../lib/labels";
import { toNumber } from "../../lib/validation";
import {
  asProtocol,
  DIRECT,
  exitOptions,
  HysteriaFields,
  type NodeFormValues,
  nodeSchema,
  RealityTargetField,
  toExitId,
} from "./nodeParts";

const STEPS = ["选服务器", "选协议", "填参数", "选出口"] as const;

const STEP_FIELDS: (keyof NodeFormValues)[][] = [
  ["server_id"],
  ["protocol"],
  ["name", "port", "address", "reality_target", "hop_start", "hop_end"],
  ["exit_id"],
];

interface NodeWizardProps {
  servers: Server[];
  exits: Exit[];
  onClose: () => void;
}

/** 创建节点向导：选服务器 → 选协议 → 填参数 → 选出口。 */
export function NodeWizard({ servers, exits, onClose }: NodeWizardProps) {
  const formId = useId();
  const [step, setStep] = useState(0);
  const [checkedTarget, setCheckedTarget] = useState<string | null>(null);
  const form = useForm<NodeFormValues>({
    resolver: zodResolver(nodeSchema),
    defaultValues: {
      server_id: servers.length === 1 ? String(servers[0]?.id) : "",
      protocol: "",
      name: "",
      port: "",
      address: "",
      reality_target: "",
      obfs: false,
      hop_enabled: false,
      hop_start: "",
      hop_end: "",
      exit_id: DIRECT,
      enabled: true,
    },
  });
  const [serverId, protocolValue, obfs, target] = useWatch({
    control: form.control,
    name: ["server_id", "protocol", "obfs", "reality_target"],
  });
  const protocol = asProtocol(protocolValue);
  const server = servers.find((s) => String(s.id) === serverId) ?? null;
  const targetReady =
    protocol !== "vless_reality" ||
    (normalizeTarget(target) !== null && checkedTarget === normalizeTarget(target));

  const create = useMutation({
    mutationFn: nodesApi.create,
    onSuccess: (node) => {
      toast.success(`已创建节点「${node.name}」`, {
        description: "记得在套餐里勾选这个节点，用户的订阅里才会有它。",
      });
      onClose();
    },
  });

  const next = async () => {
    const fields = STEP_FIELDS[step] ?? [];
    if (!(await form.trigger(fields))) {
      return;
    }
    if (step === 2 && !targetReady) {
      toast.warning("伪装目标要先检测通过");
      return;
    }
    if (step === 1 && !form.getValues("name").trim() && server && protocol) {
      form.setValue("name", `${server.name} ${protocolLabel(protocol)}`);
    }
    setStep((s) => Math.min(s + 1, STEPS.length - 1));
  };

  const submit = form.handleSubmit(async (v) => {
    const proto = asProtocol(v.protocol);
    if (!proto) {
      return;
    }
    const body: CreateNode = {
      server_id: Number(v.server_id),
      name: v.name,
      protocol: proto,
      port: toNumber(v.port),
      address: v.address || undefined,
      exit_id: toExitId(v.exit_id),
      enabled: v.enabled,
    };
    if (proto === "vless_reality") {
      body.reality_target = normalizeTarget(v.reality_target) ?? v.reality_target;
    }
    if (proto === "hysteria2") {
      body.obfs = v.obfs;
      body.hop_ports = v.hop_enabled
        ? { start: Number(v.hop_start), end: Number(v.hop_end) }
        : null;
    }
    await create.mutateAsync(body);
  });

  const hidden = protocol ? predictHiddenIn(protocol, obfs) : [];

  return (
    <Modal.Backdrop
      isOpen
      isDismissable={!create.isPending}
      onOpenChange={(open) => {
        if (!open && !create.isPending) {
          onClose();
        }
      }}
    >
      <Modal.Container size="lg" scroll="inside">
        <Modal.Dialog>
          <Modal.CloseTrigger />
          <Modal.Header>
            <Modal.Heading>添加节点</Modal.Heading>
            <ol className="mt-3 flex gap-2 text-xs">
              {STEPS.map((label, i) => (
                <li
                  key={label}
                  className={`flex items-center gap-1.5 rounded-full px-2.5 py-1 ${
                    i === step
                      ? "bg-accent-soft font-medium text-accent-soft-foreground"
                      : i < step
                        ? "text-foreground"
                        : "text-muted"
                  }`}
                >
                  <span>{i + 1}</span>
                  {label}
                </li>
              ))}
            </ol>
          </Modal.Header>
          <Modal.Body>
            <Form
              id={formId}
              className="flex flex-col gap-4 p-0.5"
              validationBehavior="aria"
              onSubmit={(e) => {
                // 前几步按回车是「下一步」，最后一步才真正创建
                if (step < STEPS.length - 1) {
                  e.preventDefault();
                  void next();
                  return;
                }
                submit(e).catch(() => {});
              }}
            >
              {step === 0 ? (
                servers.length === 0 ? (
                  <p className="text-sm text-muted">
                    还没有服务器，先到
                    <Link to="/servers" className="mx-1 text-accent hover:underline">
                      服务器页
                    </Link>
                    添加一台。
                  </p>
                ) : (
                  <SelectInput
                    control={form.control}
                    name="server_id"
                    label="服务器"
                    isRequired
                    options={servers.map((s) => ({
                      id: String(s.id),
                      label: `${s.name}（${s.address}${s.online ? "" : "，离线"}）`,
                    }))}
                    description="节点就是这台服务器上的一个入站。"
                  />
                )
              ) : null}

              {step === 1 ? (
                <RadioInput
                  control={form.control}
                  name="protocol"
                  label="协议"
                  isRequired
                  options={PROTOCOLS.map((p) => {
                    const cannot = predictHiddenIn(p.id, false);
                    return {
                      id: p.id,
                      label: p.label,
                      description: (
                        <>
                          {p.description}
                          {cannot.length > 0
                            ? `看不到这个协议的客户端：${cannot.map((f) => FORMAT_LABELS[f]).join("、")}。`
                            : "所有客户端都能用。"}
                          {(p.id === "hysteria2" || p.id === "anytls") && server
                            ? `这台服务器用${server.cert_mode === "acme" ? "自动申请的" : "自签"}证书。`
                            : ""}
                        </>
                      ),
                    };
                  })}
                />
              ) : null}

              {step === 2 ? (
                <>
                  <TextInput
                    control={form.control}
                    name="name"
                    label="节点名"
                    isRequired
                    description="订阅里显示的名字（= 和 , 会被去掉）。"
                  />
                  <div className="grid grid-cols-2 gap-4">
                    <TextInput
                      control={form.control}
                      name="port"
                      label="端口"
                      inputMode="numeric"
                      placeholder="不填就自动分配"
                      description={
                        server
                          ? `自动分配时在 ${server.port_range_start}–${server.port_range_end} 里随机选。`
                          : undefined
                      }
                    />
                    <TextInput
                      control={form.control}
                      name="address"
                      label="地址"
                      placeholder={
                        server ? `默认用服务器的 ${server.address}` : "默认用服务器的地址"
                      }
                      description="只有节点要用别的地址（例如另一个域名）时才填。"
                    />
                  </div>
                  {protocol === "vless_reality" ? (
                    <RealityTargetField
                      control={form.control}
                      serverId={server?.id ?? null}
                      checked={checkedTarget}
                      onChecked={setCheckedTarget}
                    />
                  ) : null}
                  {protocol === "hysteria2" ? <HysteriaFields control={form.control} /> : null}
                  {hidden.length > 0 ? (
                    <p className="text-xs text-warning">
                      这些客户端里看不到这个节点：{hidden.map((f) => FORMAT_LABELS[f]).join("、")}。
                    </p>
                  ) : null}
                </>
              ) : null}

              {step === 3 ? (
                <>
                  <SelectInput
                    control={form.control}
                    name="exit_id"
                    label="落地出口"
                    options={exitOptions(exits)}
                    description="走落地出口时，用户的流量从这台服务器转到出口再出去。"
                  />
                  <SwitchInput
                    control={form.control}
                    name="enabled"
                    label="创建后立即启用"
                    description="停用的节点不下发到服务器，也不进订阅。"
                  />
                </>
              ) : null}
            </Form>
          </Modal.Body>
          <Modal.Footer>
            {step > 0 ? (
              <Button
                variant="tertiary"
                className="mr-auto"
                isDisabled={create.isPending}
                onPress={() => setStep((s) => Math.max(0, s - 1))}
              >
                上一步
              </Button>
            ) : null}
            <Button slot="close" variant="tertiary" isDisabled={create.isPending}>
              取消
            </Button>
            {step < STEPS.length - 1 ? (
              <Button
                isDisabled={(step === 0 && servers.length === 0) || (step === 2 && !targetReady)}
                onPress={() => void next()}
              >
                下一步
              </Button>
            ) : (
              <ActionButton
                type="submit"
                form={formId}
                isPending={create.isPending}
                isDisabled={!targetReady}
              >
                创建节点
              </ActionButton>
            )}
          </Modal.Footer>
        </Modal.Dialog>
      </Modal.Container>
    </Modal.Backdrop>
  );
}

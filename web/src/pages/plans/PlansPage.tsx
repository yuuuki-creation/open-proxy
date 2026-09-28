import { Ellipsis, Plus } from "@gravity-ui/icons";
import { Button, Chip, Dropdown, Label, Table, toast } from "@heroui/react";
import { zodResolver } from "@hookform/resolvers/zod";
import { useMutation, useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { useForm } from "react-hook-form";
import { z } from "zod";
import { plansApi } from "../../api/endpoints";
import { nodesQuery, plansQuery } from "../../api/queries";
import type { Node, Plan, PlanFields } from "../../api/types";
import { ConfirmDialog } from "../../components/ConfirmDialog";
import { FormModal } from "../../components/FormModal";
import { CheckboxListInput, TextInput } from "../../components/form";
import { PageHeader } from "../../components/PageHeader";
import { EmptyHint, QueryView } from "../../components/QueryView";
import { bytesToGbInput, formatQuota, gbToBytes } from "../../lib/format";
import { protocolLabel } from "../../lib/labels";
import { nameSchema, optionalGbSchema, toNumber } from "../../lib/validation";

type Dialog = { kind: "create" } | { kind: "edit"; plan: Plan } | { kind: "delete"; plan: Plan };

/** 套餐：流量额度 + 可用节点。改套餐对绑定的人全部生效。 */
export function PlansPage() {
  const plans = useQuery(plansQuery);
  const nodes = useQuery(nodesQuery);
  const [dialog, setDialog] = useState<Dialog | null>(null);
  const remove = useMutation({ mutationFn: plansApi.remove });

  return (
    <>
      <PageHeader
        title="套餐"
        description="用户绑定一个套餐：每个周期的流量额度和能用的节点。改套餐对绑定的人立即生效。"
        actions={
          <Button onPress={() => setDialog({ kind: "create" })}>
            <Plus />
            添加套餐
          </Button>
        }
      />
      <QueryView query={plans}>
        {(list) => <PlanTable plans={list} nodes={nodes.data ?? []} onAction={setDialog} />}
      </QueryView>

      {dialog?.kind === "create" || dialog?.kind === "edit" ? (
        <PlanFormModal
          plan={dialog.kind === "edit" ? dialog.plan : null}
          nodes={nodes.data ?? []}
          onClose={() => setDialog(null)}
        />
      ) : null}
      {dialog?.kind === "delete" ? (
        <ConfirmDialog
          isOpen
          onOpenChange={(open) => !open && setDialog(null)}
          title={`删除套餐「${dialog.plan.name}」？`}
          confirmLabel="删除"
          onConfirm={async () => {
            await remove.mutateAsync(dialog.plan.id);
            toast.success(`已删除套餐「${dialog.plan.name}」`);
          }}
        >
          {dialog.plan.user_count > 0 ? (
            <p className="text-danger">
              有 {dialog.plan.user_count} 个用户绑定了这个套餐，要先给他们换套餐才能删除。
            </p>
          ) : (
            <p>没有用户绑定这个套餐。</p>
          )}
        </ConfirmDialog>
      ) : null}
    </>
  );
}

function PlanTable({
  plans,
  nodes,
  onAction,
}: {
  plans: Plan[];
  nodes: Node[];
  onAction: (d: Dialog) => void;
}) {
  const nodeName = new Map(nodes.map((n) => [n.id, n.name]));
  return (
    <Table>
      <Table.ScrollContainer>
        <Table.Content aria-label="套餐列表" className="min-w-[800px]">
          <Table.Header>
            <Table.Column isRowHeader>名称</Table.Column>
            <Table.Column>每周期额度</Table.Column>
            <Table.Column className="w-[45%]">可用节点</Table.Column>
            <Table.Column>绑定人数</Table.Column>
            <Table.Column className="w-14 text-end">操作</Table.Column>
          </Table.Header>
          <Table.Body
            renderEmptyState={() => <EmptyHint>还没有套餐。先建节点，再建套餐。</EmptyHint>}
          >
            {plans.map((plan) => (
              <Table.Row key={plan.id} id={plan.id}>
                <Table.Cell className="font-medium">{plan.name}</Table.Cell>
                <Table.Cell>{formatQuota(plan.traffic_quota_bytes)}</Table.Cell>
                <Table.Cell>
                  {plan.node_ids.length === 0 ? (
                    <span className="text-warning">没有节点</span>
                  ) : (
                    <div className="flex flex-wrap gap-1">
                      {plan.node_ids.map((id) => (
                        <Chip key={id} size="sm" variant="soft">
                          {nodeName.get(id) ?? `#${id}`}
                        </Chip>
                      ))}
                    </div>
                  )}
                </Table.Cell>
                <Table.Cell>{plan.user_count}</Table.Cell>
                <Table.Cell className="text-end">
                  <Dropdown>
                    <Button isIconOnly size="sm" variant="ghost" aria-label={`${plan.name} 的操作`}>
                      <Ellipsis />
                    </Button>
                    <Dropdown.Popover placement="bottom end">
                      <Dropdown.Menu
                        onAction={(key) => {
                          if (key === "edit") {
                            onAction({ kind: "edit", plan });
                          } else if (key === "delete") {
                            onAction({ kind: "delete", plan });
                          }
                        }}
                      >
                        <Dropdown.Item id="edit" textValue="编辑">
                          <Label>编辑</Label>
                        </Dropdown.Item>
                        <Dropdown.Item id="delete" textValue="删除" variant="danger">
                          <Label>删除</Label>
                        </Dropdown.Item>
                      </Dropdown.Menu>
                    </Dropdown.Popover>
                  </Dropdown>
                </Table.Cell>
              </Table.Row>
            ))}
          </Table.Body>
        </Table.Content>
      </Table.ScrollContainer>
    </Table>
  );
}

const schema = z.object({
  name: nameSchema("套餐名"),
  quota_gb: optionalGbSchema,
  node_ids: z.array(z.string()),
});

type Values = z.infer<typeof schema>;

function PlanFormModal({
  plan,
  nodes,
  onClose,
}: {
  plan: Plan | null;
  nodes: Node[];
  onClose: () => void;
}) {
  const form = useForm<Values>({
    resolver: zodResolver(schema),
    defaultValues: {
      name: plan?.name ?? "",
      quota_gb: bytesToGbInput(plan?.traffic_quota_bytes),
      node_ids: (plan?.node_ids ?? []).map(String),
    },
  });
  const create = useMutation({ mutationFn: plansApi.create });
  const update = useMutation({
    mutationFn: (body: PlanFields) => plansApi.update(plan?.id ?? 0, body),
  });

  const submit = form.handleSubmit(async (v) => {
    const quota = toNumber(v.quota_gb);
    const body: PlanFields = {
      name: v.name,
      traffic_quota_bytes: quota === undefined ? null : gbToBytes(quota),
      node_ids: v.node_ids.map(Number),
    };
    if (plan) {
      await update.mutateAsync(body);
      toast.success(`已保存套餐「${v.name}」`);
    } else {
      await create.mutateAsync(body);
      toast.success(`已添加套餐「${v.name}」`);
    }
    onClose();
  });

  const allIds = nodes.map((n) => String(n.id));

  return (
    <FormModal
      onClose={onClose}
      title={plan ? `编辑套餐「${plan.name}」` : "添加套餐"}
      description={
        plan && plan.user_count > 0
          ? `有 ${plan.user_count} 个用户绑定了这个套餐，保存后对他们立即生效。`
          : undefined
      }
      size="lg"
      isSubmitting={create.isPending || update.isPending}
      onSubmit={submit}
    >
      <div className="grid grid-cols-2 gap-4">
        <TextInput
          control={form.control}
          name="name"
          label="名称"
          isRequired
          placeholder="例如 基础版"
        />
        <TextInput
          control={form.control}
          name="quota_gb"
          label="每周期流量额度（GB）"
          inputMode="decimal"
          placeholder="不填表示不限"
          description="每个用户按自己的开通日每月重置。1 GB = 1024³ 字节。"
        />
      </div>
      {nodes.length === 0 ? (
        <p className="text-sm text-warning">还没有节点，先到节点页添加。</p>
      ) : (
        <div className="flex flex-col gap-2">
          <div className="flex gap-2">
            <Button size="sm" variant="ghost" onPress={() => form.setValue("node_ids", allIds)}>
              全选
            </Button>
            <Button size="sm" variant="ghost" onPress={() => form.setValue("node_ids", [])}>
              全不选
            </Button>
          </div>
          <CheckboxListInput
            control={form.control}
            name="node_ids"
            label="可用节点"
            options={nodes.map((n) => ({
              id: String(n.id),
              label: n.name,
              description: `${n.server_name} · ${protocolLabel(n.protocol)}${n.enabled ? "" : " · 已停用"}`,
            }))}
          />
        </div>
      )}
    </FormModal>
  );
}

// 用户列表和详情页共用的部分：状态标签、添加 / 编辑 / 续期 / 清零 / 重置凭据 / 删除的弹窗。

import { Button, Chip, Input, Label, Radio, RadioGroup, Tooltip, toast } from "@heroui/react";
import { zodResolver } from "@hookform/resolvers/zod";
import { useMutation, useQuery } from "@tanstack/react-query";
import { type ReactNode, useState } from "react";
import { useForm, useWatch } from "react-hook-form";
import { z } from "zod";
import { usersApi } from "../../api/endpoints";
import { plansQuery } from "../../api/queries";
import type { CreateUser, Plan, UpdateUser, User } from "../../api/types";
import { ConfirmDialog } from "../../components/ConfirmDialog";
import { useToday } from "../../components/DateRangeBar";
import { FormModal } from "../../components/FormModal";
import { SelectInput, SwitchInput, TextAreaInput, TextInput } from "../../components/form";
import { copyWithToast } from "../../lib/clipboard";
import { addMonths, daysBetween, isDate } from "../../lib/date";
import { formatBytes, formatQuota, formatRelative } from "../../lib/format";
import { blockedReasonLabel } from "../../lib/labels";
import { nameSchema } from "../../lib/validation";

/** 状态：正常 / 停用原因；有服务器还没同步时提示。 */
export function UserStatus({ user }: { user: User }) {
  let chip: ReactNode;
  if (user.blocked_reason) {
    chip = (
      <Tooltip delay={300}>
        <Tooltip.Trigger aria-label="停用原因">
          <Chip
            size="sm"
            variant="soft"
            color={user.blocked_reason === "manual" ? "default" : "danger"}
          >
            {blockedReasonLabel(user.blocked_reason)}
          </Chip>
        </Tooltip.Trigger>
        <Tooltip.Content>停用于 {formatRelative(user.blocked_since)}</Tooltip.Content>
      </Tooltip>
    );
  } else if (!user.enabled) {
    chip = (
      <Chip size="sm" variant="soft">
        停用中
      </Chip>
    );
  } else {
    chip = (
      <Chip size="sm" color="success" variant="soft">
        正常
      </Chip>
    );
  }
  return (
    <div className="flex flex-col items-start gap-1">
      {chip}
      {user.pending_servers > 0 ? (
        <span className="text-xs text-warning">还有 {user.pending_servers} 台服务器没同步</span>
      ) : null}
    </div>
  );
}

/** 到期日：永久 / 日期（已过期标红，快到期标黄）。 */
export function ExpiresOn({ user, today }: { user: User; today: string }) {
  if (!user.expires_on) {
    return <span className="text-muted">永久</span>;
  }
  const left = daysBetween(today, user.expires_on);
  return (
    <div className="flex flex-col">
      <span className={left < 0 ? "text-danger" : left <= 7 ? "text-warning" : undefined}>
        {user.expires_on}
      </span>
      <span className="text-xs text-muted">
        {left < 0 ? `已过期 ${-left} 天` : left === 0 ? "今天到期" : `还有 ${left} 天`}
      </span>
    </div>
  );
}

const userSchema = z.object({
  name: nameSchema("用户名"),
  remark: z.string().trim().max(200, "备注最长 200 个字"),
  plan_id: z.string().min(1, "请选择套餐"),
  started_on: z.string().refine(isDate, "请选择开通日"),
  expires_on: z.string().refine((v) => v === "" || isDate(v), "日期格式不对"),
  enabled: z.boolean(),
});

type UserValues = z.infer<typeof userSchema>;

const DURATIONS = [
  { months: 1, label: "1 个月" },
  { months: 3, label: "3 个月" },
  { months: 6, label: "半年" },
  { months: 12, label: "1 年" },
];

function UserFormModal({
  user,
  plans,
  onClose,
}: {
  user: User | null;
  plans: Plan[];
  onClose: () => void;
}) {
  const today = useToday();
  const form = useForm<UserValues>({
    resolver: zodResolver(userSchema),
    defaultValues: {
      name: user?.name ?? "",
      remark: user?.remark ?? "",
      plan_id: user ? String(user.plan_id) : plans.length === 1 ? String(plans[0]?.id) : "",
      started_on: user?.started_on ?? today,
      expires_on: user ? (user.expires_on ?? "") : addMonths(today, 1),
      enabled: user?.enabled ?? true,
    },
  });
  const startedOn = useWatch({ control: form.control, name: "started_on" });
  const create = useMutation({ mutationFn: usersApi.create });
  const update = useMutation({
    mutationFn: (body: UpdateUser) => usersApi.update(user?.id ?? 0, body),
  });

  const submit = form.handleSubmit(async (v) => {
    if (user) {
      await update.mutateAsync({
        name: v.name,
        remark: v.remark,
        plan_id: Number(v.plan_id),
        started_on: v.started_on,
        expires_on: v.expires_on || null,
        enabled: v.enabled,
      });
      toast.success(`已保存用户「${v.name}」`);
    } else {
      const body: CreateUser = {
        name: v.name,
        remark: v.remark,
        plan_id: Number(v.plan_id),
        started_on: v.started_on,
        expires_on: v.expires_on || null,
        enabled: v.enabled,
      };
      const created = await create.mutateAsync(body);
      toast.success(`已添加用户「${created.name}」`, {
        description: created.sub_url
          ? "在用户列表里点「复制订阅链接」发给他。"
          : "主控还没有域名，先在设置里填写，才能生成订阅链接。",
      });
    }
    onClose();
  });

  return (
    <FormModal
      onClose={onClose}
      title={user ? `编辑用户「${user.name}」` : "添加用户"}
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
          placeholder="例如 小明"
        />
        <SelectInput
          control={form.control}
          name="plan_id"
          label="套餐"
          isRequired
          options={plans.map((p) => ({
            id: String(p.id),
            label: `${p.name}（${formatQuota(p.traffic_quota_bytes)}，${p.node_ids.length} 个节点）`,
          }))}
          description={plans.length === 0 ? "还没有套餐，先到套餐页添加。" : "换套餐立即生效。"}
        />
        <TextInput
          control={form.control}
          name="started_on"
          label="开通日"
          type="date"
          isRequired
          description="每月这一天重置本周期用量（短月按月末）。"
        />
        <div className="flex flex-col gap-2">
          <TextInput
            control={form.control}
            name="expires_on"
            label="到期日"
            type="date"
            description="这一天结束（管理员时区 24 点）时停用；不填表示永久。"
          />
          <div className="flex flex-wrap gap-1">
            {DURATIONS.map((d) => (
              <Button
                key={d.months}
                size="sm"
                variant="ghost"
                onPress={() =>
                  form.setValue(
                    "expires_on",
                    addMonths(isDate(startedOn) ? startedOn : today, d.months),
                    {
                      shouldValidate: true,
                    },
                  )
                }
              >
                {d.label}
              </Button>
            ))}
            <Button
              size="sm"
              variant="ghost"
              onPress={() => form.setValue("expires_on", "", { shouldValidate: true })}
            >
              永久
            </Button>
          </div>
        </div>
      </div>
      <TextAreaInput
        control={form.control}
        name="remark"
        label="备注"
        rows={2}
        placeholder="只有管理员看得到"
      />
      <SwitchInput
        control={form.control}
        name="enabled"
        label="启用"
        description="手动停用后他的所有节点都连不上；到期和超额会自动停用，不用手动关。"
      />
    </FormModal>
  );
}

/** 续期：在「今天」和「原到期日」里较晚的那天基础上加时长，或者改成永久。 */
function RenewModal({ user, onClose }: { user: User; onClose: () => void }) {
  const today = useToday();
  const base = user.expires_on && user.expires_on > today ? user.expires_on : today;
  const [choice, setChoice] = useState("1");
  const [custom, setCustom] = useState(user.expires_on ?? "");
  const renew = useMutation({
    mutationFn: (expiresOn: string | null) => usersApi.update(user.id, { expires_on: expiresOn }),
  });
  const target =
    choice === "forever" ? null : choice === "custom" ? custom : addMonths(base, Number(choice));
  const valid = target === null || isDate(target);

  return (
    <FormModal
      onClose={onClose}
      title={`给「${user.name}」续期`}
      description={
        user.expires_on
          ? `现在的到期日是 ${user.expires_on}${user.expires_on < today ? "（已过期）" : ""}。`
          : "现在是永久。"
      }
      submitLabel="续期"
      isSubmitting={renew.isPending}
      isSubmitDisabled={!valid}
      onSubmit={async (event) => {
        event.preventDefault();
        if (!valid) {
          return;
        }
        await renew.mutateAsync(target);
        toast.success(target ? `已续期到 ${target}` : "已改成永久");
        onClose();
      }}
    >
      <RadioGroup value={choice} onChange={setChoice}>
        <Label>续多久</Label>
        {[
          ...DURATIONS.map((d) => ({
            id: String(d.months),
            label: `${d.label}（到 ${addMonths(base, d.months)}）`,
          })),
          { id: "custom", label: "指定到期日" },
          { id: "forever", label: "永久" },
        ].map((option) => (
          <Radio key={option.id} value={option.id}>
            <Radio.Content>
              <Radio.Control>
                <Radio.Indicator />
              </Radio.Control>
              {option.label}
            </Radio.Content>
          </Radio>
        ))}
      </RadioGroup>
      {choice === "custom" ? (
        <Input
          type="date"
          aria-label="到期日"
          className="w-48"
          value={custom}
          onChange={(e) => setCustom(e.target.value)}
        />
      ) : null}
      <p className="text-xs text-muted">
        从 {base === today ? "今天" : `原到期日 ${base}`} 往后算。因为到期停用的，续期后自动恢复。
      </p>
    </FormModal>
  );
}

type Dialog =
  | { kind: "create" }
  | { kind: "edit"; user: User }
  | { kind: "renew"; user: User }
  | { kind: "reset-period"; user: User }
  | { kind: "reset-credentials"; user: User }
  | { kind: "delete"; user: User };

/** 用户相关的弹窗和操作：open 打开弹窗，dialog 放进页面里渲染。 */
export function useUserDialogs(options: { onDeleted?: () => void } = {}) {
  const [dialog, setDialog] = useState<Dialog | null>(null);
  const plans = useQuery(plansQuery);
  const close = () => setDialog(null);

  const resetPeriod = useMutation({ mutationFn: usersApi.resetPeriod });
  const resetCredentials = useMutation({ mutationFn: usersApi.resetCredentials });
  const remove = useMutation({ mutationFn: usersApi.remove });
  const setEnabled = useMutation({
    mutationFn: ({ user, enabled }: { user: User; enabled: boolean }) =>
      usersApi.update(user.id, { enabled }),
    onSuccess: (saved) => {
      toast.success(saved.enabled ? `已启用「${saved.name}」` : `已停用「${saved.name}」`, {
        description: "正在同步到各服务器，列表里会显示还有几台没同步。",
      });
    },
  });

  const copyLink = (user: User) => {
    if (!user.sub_url) {
      toast.warning("主控还没有域名，先在设置里填写主控域名，才能生成订阅链接。");
      return;
    }
    void copyWithToast(user.sub_url, "订阅链接");
  };

  let node: ReactNode = null;
  switch (dialog?.kind) {
    case "create":
    case "edit":
      node = (
        <UserFormModal
          user={dialog.kind === "edit" ? dialog.user : null}
          plans={plans.data ?? []}
          onClose={close}
        />
      );
      break;
    case "renew":
      node = <RenewModal user={dialog.user} onClose={close} />;
      break;
    case "reset-period": {
      const user = dialog.user;
      node = (
        <ConfirmDialog
          isOpen
          onOpenChange={(open) => !open && close()}
          title={`清零「${user.name}」本周期的用量？`}
          confirmLabel="清零"
          status="warning"
          onConfirm={async () => {
            await resetPeriod.mutateAsync(user.id);
            toast.success(`已清零「${user.name}」本周期的用量`);
          }}
        >
          <p>
            本周期已用 {formatBytes(user.used_bytes)} 会清零；重置日不变，历史流量和每日记录不变。
          </p>
          {user.blocked_reason === "over_quota" ? (
            <p>他现在因为超额停用，清零后自动恢复。</p>
          ) : null}
        </ConfirmDialog>
      );
      break;
    }
    case "reset-credentials": {
      const user = dialog.user;
      node = (
        <ConfirmDialog
          isOpen
          onOpenChange={(open) => !open && close()}
          title={`重置「${user.name}」的订阅链接和凭据？`}
          confirmLabel="重置"
          onConfirm={async () => {
            await resetCredentials.mutateAsync(user.id);
            toast.success("已重置订阅链接和凭据", {
              description: "把新的订阅链接发给他重新导入。",
            });
          }}
        >
          <p>旧的订阅链接和所有节点凭据立即失效，他要用新的订阅链接重新导入才能继续用。</p>
          <p>一般在链接泄露时使用。</p>
        </ConfirmDialog>
      );
      break;
    }
    case "delete": {
      const user = dialog.user;
      node = (
        <ConfirmDialog
          isOpen
          onOpenChange={(open) => !open && close()}
          title={`删除用户「${user.name}」？`}
          confirmLabel="删除"
          onConfirm={async () => {
            await remove.mutateAsync(user.id);
            toast.success(`已删除用户「${user.name}」`);
            options.onDeleted?.();
          }}
        >
          <p>他的凭据和订阅链接立即失效。</p>
          <p className="text-danger">他的流量记录会一起删除，统计里的历史总量随之变小。</p>
        </ConfirmDialog>
      );
      break;
    }
    default:
      node = null;
  }

  return {
    open: setDialog,
    dialog: node,
    copyLink,
    toggleEnabled: (user: User) => setEnabled.mutate({ user, enabled: !user.enabled }),
  };
}

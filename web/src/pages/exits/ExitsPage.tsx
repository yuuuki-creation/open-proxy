import { Ellipsis, Eye, EyeSlash, Plus } from "@gravity-ui/icons";
import { Button, Chip, Dropdown, Label, Table, toast } from "@heroui/react";
import { zodResolver } from "@hookform/resolvers/zod";
import { useMutation, useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { useForm, useWatch } from "react-hook-form";
import { z } from "zod";
import { exitsApi } from "../../api/endpoints";
import { exitsQuery, serversQuery } from "../../api/queries";
import type { CreateExit, Exit, Server, UpdateExit } from "../../api/types";
import { ConfirmDialog } from "../../components/ConfirmDialog";
import { CopyIconButton } from "../../components/CopyBlock";
import { FormModal } from "../../components/FormModal";
import { RadioInput, SelectInput, TextInput } from "../../components/form";
import { PageHeader } from "../../components/PageHeader";
import { EmptyHint, QueryView } from "../../components/QueryView";
import { EXIT_KIND_LABELS } from "../../lib/labels";
import { isHostname, isIpv4, nameSchema, optionalPortSchema, toNumber } from "../../lib/validation";

type Dialog = { kind: "create" } | { kind: "edit"; exit: Exit } | { kind: "delete"; exit: Exit };

/** 落地出口：登记第三方 SOCKS5，或者选一台服务器建成自建落地机（账号密码由主控生成）。 */
export function ExitsPage() {
  const exits = useQuery(exitsQuery);
  const servers = useQuery(serversQuery);
  const [dialog, setDialog] = useState<Dialog | null>(null);
  const remove = useMutation({ mutationFn: exitsApi.remove });

  return (
    <>
      <PageHeader
        title="落地出口"
        description="节点可以把用户的流量转到落地出口再出去（例如为了换出口 IP）。一个节点固定走一个出口。"
        actions={
          <Button onPress={() => setDialog({ kind: "create" })}>
            <Plus />
            添加出口
          </Button>
        }
      />
      <QueryView query={exits}>
        {(list) => <ExitTable exits={list} onAction={setDialog} />}
      </QueryView>

      {dialog?.kind === "create" || dialog?.kind === "edit" ? (
        <ExitFormModal
          exit={dialog.kind === "edit" ? dialog.exit : null}
          servers={servers.data ?? []}
          onClose={() => setDialog(null)}
        />
      ) : null}
      {dialog?.kind === "delete" ? (
        <ConfirmDialog
          isOpen
          onOpenChange={(open) => !open && setDialog(null)}
          title={`删除出口「${dialog.exit.name}」？`}
          confirmLabel="删除"
          onConfirm={async () => {
            await remove.mutateAsync(dialog.exit.id);
            toast.success(`已删除出口「${dialog.exit.name}」`);
          }}
        >
          {dialog.exit.node_count > 0 ? (
            <p className="text-danger">
              有 {dialog.exit.node_count} 个节点在用这个出口，要先在节点页给它们换出口才能删除。
            </p>
          ) : (
            <p>没有节点在用这个出口。</p>
          )}
          {dialog.exit.kind === "self_built" ? (
            <p>落地机「{dialog.exit.landing_server_name}」上的 SOCKS5 入站会随后删除。</p>
          ) : null}
        </ConfirmDialog>
      ) : null}
    </>
  );
}

function Secret({ value }: { value: string }) {
  const [shown, setShown] = useState(false);
  if (!value) {
    return <span className="text-muted">无</span>;
  }
  return (
    <span className="inline-flex items-center gap-1">
      <span className="font-mono text-xs">{shown ? value : "••••••••"}</span>
      <Button
        isIconOnly
        size="sm"
        variant="ghost"
        aria-label={shown ? "隐藏密码" : "显示密码"}
        onPress={() => setShown((s) => !s)}
      >
        {shown ? <EyeSlash className="size-4" /> : <Eye className="size-4" />}
      </Button>
      <CopyIconButton value={value} what="密码" label="复制密码" />
    </span>
  );
}

function ExitTable({ exits, onAction }: { exits: Exit[]; onAction: (d: Dialog) => void }) {
  return (
    <Table>
      <Table.ScrollContainer>
        <Table.Content aria-label="落地出口列表" className="min-w-[900px]">
          <Table.Header>
            <Table.Column isRowHeader>名称</Table.Column>
            <Table.Column>种类</Table.Column>
            <Table.Column>地址</Table.Column>
            <Table.Column>账号</Table.Column>
            <Table.Column>密码</Table.Column>
            <Table.Column>在用的节点</Table.Column>
            <Table.Column className="w-14 text-end">操作</Table.Column>
          </Table.Header>
          <Table.Body
            renderEmptyState={() => <EmptyHint>还没有落地出口，节点默认直连。</EmptyHint>}
          >
            {exits.map((exit) => (
              <Table.Row key={exit.id} id={exit.id}>
                <Table.Cell className="font-medium">{exit.name}</Table.Cell>
                <Table.Cell>
                  <div className="flex flex-col gap-0.5">
                    <Chip
                      size="sm"
                      variant="soft"
                      color={exit.kind === "self_built" ? "accent" : "default"}
                    >
                      {EXIT_KIND_LABELS[exit.kind]}
                    </Chip>
                    {exit.landing_server_name ? (
                      <span className="text-xs text-muted">落地机：{exit.landing_server_name}</span>
                    ) : null}
                  </div>
                </Table.Cell>
                <Table.Cell className="font-mono text-xs">
                  {exit.host}:{exit.port}
                </Table.Cell>
                <Table.Cell className="font-mono text-xs">{exit.username || "无"}</Table.Cell>
                <Table.Cell>
                  <Secret value={exit.password} />
                </Table.Cell>
                <Table.Cell>{exit.node_count}</Table.Cell>
                <Table.Cell className="text-end">
                  <Dropdown>
                    <Button isIconOnly size="sm" variant="ghost" aria-label={`${exit.name} 的操作`}>
                      <Ellipsis />
                    </Button>
                    <Dropdown.Popover placement="bottom end">
                      <Dropdown.Menu
                        onAction={(key) => {
                          if (key === "edit") {
                            onAction({ kind: "edit", exit });
                          } else if (key === "delete") {
                            onAction({ kind: "delete", exit });
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

const schema = z
  .object({
    kind: z.enum(["third_party", "self_built"]),
    name: nameSchema("出口名"),
    host: z.string().trim(),
    port: optionalPortSchema,
    username: z.string().max(255, "最长 255 个字符"),
    password: z.string().max(255, "最长 255 个字符"),
    landing_server_id: z.string(),
  })
  .superRefine((v, ctx) => {
    if (v.kind === "third_party") {
      if (!isIpv4(v.host) && !isHostname(v.host)) {
        ctx.addIssue({ code: "custom", message: "要写 IPv4 地址或域名", path: ["host"] });
      }
      if (!v.port) {
        ctx.addIssue({ code: "custom", message: "请填写端口", path: ["port"] });
      }
    } else if (!v.landing_server_id) {
      ctx.addIssue({ code: "custom", message: "请选择落地机", path: ["landing_server_id"] });
    }
  });

type Values = z.infer<typeof schema>;

function ExitFormModal({
  exit,
  servers,
  onClose,
}: {
  exit: Exit | null;
  servers: Server[];
  onClose: () => void;
}) {
  const form = useForm<Values>({
    resolver: zodResolver(schema),
    defaultValues: {
      kind: exit?.kind ?? "third_party",
      name: exit?.name ?? "",
      host: exit?.kind === "third_party" ? exit.host : "",
      port: exit ? String(exit.port) : "",
      username: exit?.username ?? "",
      password: exit?.password ?? "",
      landing_server_id: exit?.landing_server_id ? String(exit.landing_server_id) : "",
    },
  });
  const kind = useWatch({ control: form.control, name: "kind" });
  const create = useMutation({ mutationFn: exitsApi.create });
  const update = useMutation({
    mutationFn: (body: UpdateExit) => exitsApi.update(exit?.id ?? 0, body),
  });

  const submit = form.handleSubmit(async (v) => {
    if (exit) {
      const body: UpdateExit = { name: v.name };
      const port = toNumber(v.port);
      if (port !== undefined) {
        body.port = port;
      }
      if (exit.kind === "third_party") {
        body.host = v.host;
        body.username = v.username;
        body.password = v.password;
      }
      await update.mutateAsync(body);
      toast.success(`已保存出口「${v.name}」`);
    } else {
      const body: CreateExit =
        v.kind === "third_party"
          ? {
              name: v.name,
              kind: v.kind,
              host: v.host,
              port: toNumber(v.port),
              username: v.username,
              password: v.password,
            }
          : {
              name: v.name,
              kind: v.kind,
              landing_server_id: Number(v.landing_server_id),
              port: toNumber(v.port),
            };
      await create.mutateAsync(body);
      toast.success(`已添加出口「${v.name}」`);
    }
    onClose();
  });

  return (
    <FormModal
      onClose={onClose}
      title={exit ? `编辑出口「${exit.name}」` : "添加落地出口"}
      isSubmitting={create.isPending || update.isPending}
      onSubmit={submit}
    >
      {exit ? null : (
        <RadioInput
          control={form.control}
          name="kind"
          label="种类"
          orientation="horizontal"
          options={[
            {
              id: "third_party",
              label: "第三方 SOCKS5",
              description: "登记别人提供的 SOCKS5 代理。",
            },
            {
              id: "self_built",
              label: "自建落地机",
              description: "选一台自己的服务器，主控在上面建 SOCKS5 入站，账号密码自动生成。",
            },
          ]}
        />
      )}
      <TextInput
        control={form.control}
        name="name"
        label="名称"
        isRequired
        placeholder="例如 美国家宽"
      />
      {kind === "third_party" ? (
        <>
          <div className="grid grid-cols-3 gap-4">
            <TextInput
              className="col-span-2"
              control={form.control}
              name="host"
              label="地址"
              isRequired
              placeholder="IPv4 或域名"
            />
            <TextInput
              control={form.control}
              name="port"
              label="端口"
              isRequired
              inputMode="numeric"
            />
          </div>
          <div className="grid grid-cols-2 gap-4">
            <TextInput
              control={form.control}
              name="username"
              label="账号"
              placeholder="没有就不填"
            />
            <TextInput
              control={form.control}
              name="password"
              label="密码"
              type="password"
              placeholder="没有就不填"
            />
          </div>
        </>
      ) : (
        <>
          {exit ? (
            <p className="text-sm text-muted">
              落地机：{exit.landing_server_name}（{exit.host}）。账号密码由主控生成，不能修改。
            </p>
          ) : (
            <SelectInput
              control={form.control}
              name="landing_server_id"
              label="落地机"
              isRequired
              options={servers.map((s) => ({
                id: String(s.id),
                label: `${s.name}（${s.address}）`,
              }))}
              description="一台服务器只能当一个出口的落地机。只允许用这个出口的节点所在服务器连进来。"
            />
          )}
          <TextInput
            control={form.control}
            name="port"
            label="SOCKS5 端口"
            inputMode="numeric"
            placeholder="不填就在落地机的端口范围里自动分配"
          />
        </>
      )}
    </FormModal>
  );
}

import { Ellipsis, Grip, Plus } from "@gravity-ui/icons";
import { Button, Chip, Dropdown, Label, Switch, Table, toast } from "@heroui/react";
import { useMutation, useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { useDragAndDrop } from "react-aria-components";
import { nodesApi } from "../../api/endpoints";
import { exitsQuery, nodesQuery, serversQuery } from "../../api/queries";
import type { Node } from "../../api/types";
import { ConfirmDialog } from "../../components/ConfirmDialog";
import { PageHeader } from "../../components/PageHeader";
import { EmptyHint, ErrorBlock, LoadingBlock } from "../../components/QueryView";
import { protocolLabel } from "../../lib/labels";
import { queryClient } from "../../queryClient";
import { NodeEditModal } from "./NodeEditModal";
import { NodeWizard } from "./NodeWizard";
import { HiddenIn } from "./nodeParts";

type Dialog = { kind: "create" } | { kind: "edit"; node: Node } | { kind: "delete"; node: Node };

/** 节点列表：拖拽调整在订阅里的顺序，开关启用 / 停用。 */
export function NodesPage() {
  const nodes = useQuery(nodesQuery);
  const servers = useQuery(serversQuery);
  const exits = useQuery(exitsQuery);
  const [dialog, setDialog] = useState<Dialog | null>(null);
  const remove = useMutation({ mutationFn: nodesApi.remove });

  const ready = nodes.data !== undefined && servers.data !== undefined && exits.data !== undefined;

  return (
    <>
      <PageHeader
        title="节点"
        description="一个节点是一台服务器上的一个入站。拖动左边的把手调整在订阅里的顺序。"
        actions={
          <Button isDisabled={!ready} onPress={() => setDialog({ kind: "create" })}>
            <Plus />
            添加节点
          </Button>
        }
      />
      {nodes.isError ? (
        <ErrorBlock error={nodes.error} onRetry={() => void nodes.refetch()} />
      ) : nodes.data === undefined ? (
        <LoadingBlock />
      ) : (
        <NodeTable nodes={nodes.data} onAction={setDialog} />
      )}

      {dialog?.kind === "create" && ready ? (
        <NodeWizard
          servers={servers.data ?? []}
          exits={exits.data ?? []}
          onClose={() => setDialog(null)}
        />
      ) : null}
      {dialog?.kind === "edit" ? (
        <NodeEditModal
          node={dialog.node}
          exits={exits.data ?? []}
          onClose={() => setDialog(null)}
        />
      ) : null}
      {dialog?.kind === "delete" ? (
        <ConfirmDialog
          isOpen
          onOpenChange={(open) => !open && setDialog(null)}
          title={`删除节点「${dialog.node.name}」？`}
          confirmLabel="删除"
          onConfirm={async () => {
            await remove.mutateAsync(dialog.node.id);
            toast.success(`已删除节点「${dialog.node.name}」`);
          }}
        >
          <p>节点会从所有套餐里移除，用户的订阅里也不再有它；服务器上的入站随后删除。</p>
          <p>这个节点的历史流量保留，统计里显示为「已删除」。</p>
        </ConfirmDialog>
      ) : null}
    </>
  );
}

function NodeTable({ nodes, onAction }: { nodes: Node[]; onAction: (d: Dialog) => void }) {
  const toggle = useMutation({
    mutationFn: ({ id, enabled }: { id: number; enabled: boolean }) =>
      nodesApi.update(id, { enabled }),
  });
  const setOrder = useMutation({
    mutationFn: nodesApi.setOrder,
    onError: () => {
      // 保存顺序失败：重新拉取，恢复成主控里的顺序
      void queryClient.invalidateQueries({ queryKey: nodesQuery.queryKey });
    },
  });

  const { dragAndDropHooks } = useDragAndDrop({
    getItems: (keys) => [...keys].map((key) => ({ "text/plain": String(key) })),
    onReorder: (event) => {
      const moving = [...event.keys].map(Number);
      const targetId = Number(event.target.key);
      const rest = nodes.filter((n) => !moving.includes(n.id));
      let index = rest.findIndex((n) => n.id === targetId);
      if (index < 0) {
        return;
      }
      if (event.target.dropPosition === "after") {
        index += 1;
      }
      const moved = nodes.filter((n) => moving.includes(n.id));
      const next = [...rest.slice(0, index), ...moved, ...rest.slice(index)];
      // 先在界面上换好顺序，再提交整个顺序
      queryClient.setQueryData(nodesQuery.queryKey, next);
      setOrder.mutate(next.map((n) => n.id));
    },
  });

  return (
    <Table>
      <Table.ScrollContainer>
        <Table.Content
          aria-label="节点列表"
          className="min-w-[1000px]"
          dragAndDropHooks={dragAndDropHooks}
        >
          <Table.Header>
            <Table.Column className="w-10">
              <span className="sr-only">拖动排序</span>
            </Table.Column>
            <Table.Column isRowHeader>名称</Table.Column>
            <Table.Column>协议</Table.Column>
            <Table.Column>服务器 / 地址</Table.Column>
            <Table.Column>落地出口</Table.Column>
            <Table.Column>看不到的客户端</Table.Column>
            <Table.Column>启用</Table.Column>
            <Table.Column className="w-14 text-end">操作</Table.Column>
          </Table.Header>
          <Table.Body
            renderEmptyState={() => (
              <EmptyHint>还没有节点。先添加服务器，再点右上角「添加节点」。</EmptyHint>
            )}
          >
            {nodes.map((node) => (
              <Table.Row key={node.id} id={node.id}>
                <Table.Cell>
                  <Button
                    slot="drag"
                    isIconOnly
                    size="sm"
                    variant="ghost"
                    aria-label={`拖动「${node.name}」调整顺序`}
                    className="cursor-grab"
                  >
                    <Grip className="size-4 text-muted" />
                  </Button>
                </Table.Cell>
                <Table.Cell className="font-medium">{node.name}</Table.Cell>
                <Table.Cell>
                  <div className="flex flex-col gap-1">
                    <span>{protocolLabel(node.protocol)}</span>
                    <span className="flex gap-1">
                      {node.obfs ? (
                        <Chip size="sm" variant="soft">
                          混淆
                        </Chip>
                      ) : null}
                      {node.hop_ports ? (
                        <Chip size="sm" variant="soft">
                          端口跳跃
                        </Chip>
                      ) : null}
                    </span>
                  </div>
                </Table.Cell>
                <Table.Cell>
                  <div className="flex flex-col">
                    <span>{node.server_name}</span>
                    <span className="font-mono text-xs text-muted">
                      {node.address}:{node.port}
                      {node.hop_ports
                        ? `（跳跃 ${node.hop_ports.start}–${node.hop_ports.end}）`
                        : ""}
                    </span>
                    {node.reality ? (
                      <span className="font-mono text-xs text-muted">
                        伪装 {node.reality.target}
                      </span>
                    ) : null}
                  </div>
                </Table.Cell>
                <Table.Cell>
                  {node.exit_name ?? <span className="text-muted">直连</span>}
                </Table.Cell>
                <Table.Cell>
                  <HiddenIn formats={node.hidden_in} />
                </Table.Cell>
                <Table.Cell>
                  <Switch
                    size="sm"
                    aria-label={`启用「${node.name}」`}
                    isSelected={node.enabled}
                    isDisabled={toggle.isPending && toggle.variables?.id === node.id}
                    onChange={(enabled) => toggle.mutate({ id: node.id, enabled })}
                  >
                    <Switch.Content>
                      <Switch.Control>
                        <Switch.Thumb />
                      </Switch.Control>
                    </Switch.Content>
                  </Switch>
                </Table.Cell>
                <Table.Cell className="text-end">
                  <Dropdown>
                    <Button isIconOnly size="sm" variant="ghost" aria-label={`${node.name} 的操作`}>
                      <Ellipsis />
                    </Button>
                    <Dropdown.Popover placement="bottom end">
                      <Dropdown.Menu
                        onAction={(key) => {
                          if (key === "edit") {
                            onAction({ kind: "edit", node });
                          } else if (key === "delete") {
                            onAction({ kind: "delete", node });
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

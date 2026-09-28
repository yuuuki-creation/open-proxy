// 服务器列表和详情页共用的部分：状态标签、版本、同步状态，以及添加 / 编辑 / 安装命令 / 升级 / 删除的弹窗。

import { ArrowDown, ArrowUp, CircleCheck, TriangleExclamation } from "@gravity-ui/icons";
import { Alert, Button, Chip, Modal, Spinner, Tooltip, toast } from "@heroui/react";
import { useMutation, useQuery } from "@tanstack/react-query";
import { type ReactNode, useState } from "react";
import { serversApi } from "../../api/endpoints";
import { exitsQuery, nodesQuery, settingsQuery } from "../../api/queries";
import type { ApplyFailure, Exit, Node, Server } from "../../api/types";
import { ConfirmDialog } from "../../components/ConfirmDialog";
import { CopyBlock } from "../../components/CopyBlock";
import { formatRelative, formatSpeed } from "../../lib/format";
import { APPLY_ITEM_LABELS } from "../../lib/labels";
import { ServerFormModal } from "./ServerFormModal";

/** Agent 版本和主控不同（并且 Agent 连上过）就可以升级。 */
export function canUpgrade(server: Server, masterVersion: string | undefined): boolean {
  return Boolean(masterVersion && server.agent_version && server.agent_version !== masterVersion);
}

export function ServerStatusChip({ server }: { server: Server }) {
  if (!server.online) {
    return (
      <Tooltip delay={300}>
        <Tooltip.Trigger aria-label="离线">
          <Chip size="sm" variant="soft">
            离线
          </Chip>
        </Tooltip.Trigger>
        <Tooltip.Content>最近一次连接：{formatRelative(server.last_seen_at)}</Tooltip.Content>
      </Tooltip>
    );
  }
  if (server.version_mismatch) {
    return (
      <Tooltip delay={300}>
        <Tooltip.Trigger aria-label="版本不一致">
          <Chip size="sm" color="warning" variant="soft">
            版本不一致
          </Chip>
        </Tooltip.Trigger>
        <Tooltip.Content>
          Agent 和主控版本不一致：按本地保存的配置继续服务，暂停同步，升级后恢复。
        </Tooltip.Content>
      </Tooltip>
    );
  }
  return (
    <Chip size="sm" color="success" variant="soft">
      在线
    </Chip>
  );
}

export function AgentVersion({
  server,
  masterVersion,
}: {
  server: Server;
  masterVersion: string | undefined;
}) {
  if (!server.agent_version) {
    return <span className="text-muted">还没连上过</span>;
  }
  return (
    <div className="flex flex-col gap-1">
      <div className="flex items-center gap-1.5">
        <span className="font-mono text-xs">{server.agent_version}</span>
        {server.agent_arch ? <span className="text-xs text-muted">{server.agent_arch}</span> : null}
        {canUpgrade(server, masterVersion) ? (
          <Chip size="sm" color="accent" variant="soft">
            可升级
          </Chip>
        ) : null}
      </div>
      {server.rolled_back_from ? (
        <span className="text-xs text-warning">
          上次升级到 {server.rolled_back_from} 失败，已回滚
        </span>
      ) : null}
      {server.upgrade_error ? (
        <span className="max-w-64 text-xs text-danger">升级失败：{server.upgrade_error}</span>
      ) : null}
    </div>
  );
}

export function SpeedCell({ server }: { server: Server }) {
  if (!server.online) {
    return <span className="text-muted">-</span>;
  }
  return (
    <div className="flex flex-col gap-0.5 text-xs tabular-nums">
      <span className="inline-flex items-center gap-1">
        <ArrowDown className="size-3 text-muted" />
        {formatSpeed(server.rx_speed)}
      </span>
      <span className="inline-flex items-center gap-1">
        <ArrowUp className="size-3 text-muted" />
        {formatSpeed(server.tx_speed)}
      </span>
    </div>
  );
}

/** 失败项对应的对象名：节点、出口的名字，或者「证书」这类整体项。 */
export function failureTarget(failure: ApplyFailure, nodes: Node[], exits: Exit[]): string {
  if (failure.item === "node" || failure.item === "port_hopping") {
    const node = nodes.find((n) => n.id === failure.id);
    return node ? `节点「${node.name}」` : `节点 #${failure.id}`;
  }
  if (failure.item === "exit") {
    const exit = exits.find((e) => e.id === failure.id);
    return exit ? `出口「${exit.name}」` : `出口 #${failure.id}`;
  }
  return APPLY_ITEM_LABELS[failure.item] ?? failure.item;
}

export function FailureList({ failures }: { failures: ApplyFailure[] }) {
  const nodes = useQuery(nodesQuery);
  const exits = useQuery(exitsQuery);
  return (
    <ul className="flex flex-col gap-1.5">
      {failures.map((f) => (
        <li key={`${f.item}-${f.id}-${f.reason}`} className="text-sm">
          <span className="font-medium">
            {failureTarget(f, nodes.data ?? [], exits.data ?? [])}
            {f.item === "port_hopping" ? "的端口跳跃" : ""}
          </span>
          ：{f.reason}
        </li>
      ))}
    </ul>
  );
}

/** 配置同步状态：已同步 / 同步中 / 有失败项 / 离线暂停。 */
export function SyncStatus({ server }: { server: Server }) {
  const failures = server.apply_failures ?? [];
  if (failures.length > 0) {
    return (
      <Tooltip delay={200}>
        <Tooltip.Trigger aria-label="应用失败的项">
          <Chip size="sm" color="danger" variant="soft">
            <TriangleExclamation className="size-3.5" />
            <Chip.Label>{failures.length} 项失败</Chip.Label>
          </Chip>
        </Tooltip.Trigger>
        <Tooltip.Content className="max-w-sm">
          <FailureList failures={failures} />
        </Tooltip.Content>
      </Tooltip>
    );
  }
  if (server.version_mismatch) {
    return <span className="text-xs text-warning">暂停同步</span>;
  }
  if (!server.agent_version) {
    // 还没装好 Agent：谈不上同步
    return <span className="text-xs text-muted">等 Agent 连上</span>;
  }
  if (server.synced) {
    return (
      <span className="inline-flex items-center gap-1 text-xs text-success">
        <CircleCheck className="size-3.5" />
        已同步
      </span>
    );
  }
  if (!server.online) {
    return <span className="text-xs text-muted">等 Agent 连上后同步</span>;
  }
  return (
    <span className="inline-flex items-center gap-1.5 text-xs text-muted">
      <Spinner size="sm" />
      同步中
    </span>
  );
}

export function InstallCommandModal({
  serverName,
  command,
  onClose,
}: {
  serverName: string;
  command: string;
  onClose: () => void;
}) {
  return (
    <Modal.Backdrop
      isOpen
      isDismissable={false}
      onOpenChange={(open) => {
        if (!open) {
          onClose();
        }
      }}
    >
      <Modal.Container size="lg">
        <Modal.Dialog>
          <Modal.Header>
            <Modal.Heading>安装 Agent：{serverName}</Modal.Heading>
          </Modal.Header>
          <Modal.Body className="flex flex-col gap-4">
            <Alert status="warning">
              <Alert.Indicator />
              <Alert.Content>
                <Alert.Title>安装命令只显示这一次，请现在复制</Alert.Title>
                <Alert.Description>
                  <span>命令里带着这台服务器的 Agent Token，主控只保存它的哈希。</span>
                  <span>关掉这个窗口后就看不到了，需要时只能重新生成（旧的立即失效）。</span>
                </Alert.Description>
              </Alert.Content>
            </Alert>
            <p className="text-sm">用 root 在这台服务器上执行：</p>
            <CopyBlock value={command} what="安装命令" />
            <p className="text-sm text-muted">
              装好后 Agent 会自动连上主控，服务器列表里会显示「在线」（每 10 秒刷新一次）。
            </p>
          </Modal.Body>
          <Modal.Footer>
            <Button slot="close">我已复制，关闭</Button>
          </Modal.Footer>
        </Modal.Dialog>
      </Modal.Container>
    </Modal.Backdrop>
  );
}

type Dialog =
  | { kind: "create" }
  | { kind: "edit"; server: Server }
  | { kind: "regenerate"; server: Server }
  | { kind: "command"; serverName: string; command: string }
  | { kind: "upgrade"; server: Server }
  | { kind: "upgrade-all"; servers: Server[] }
  | { kind: "delete"; server: Server };

/** 服务器相关的弹窗：open 打开某个弹窗，dialog 放进页面里渲染。 */
export function useServerDialogs(options: { onDeleted?: () => void } = {}) {
  const [dialog, setDialog] = useState<Dialog | null>(null);
  const settings = useQuery(settingsQuery);
  const version = settings.data?.version;

  // 只关掉指定的弹窗：确认框完成后可能已经换成了下一个弹窗（例如显示新的安装命令）
  const closeIf = (kind: Dialog["kind"]) =>
    setDialog((current) => (current?.kind === kind ? null : current));

  const regenerate = useMutation({
    mutationFn: serversApi.installCommand,
    meta: { keepCache: true },
  });
  const upgrade = useMutation({ mutationFn: serversApi.upgrade });
  const upgradeAll = useMutation({ mutationFn: serversApi.upgradeAll });
  const remove = useMutation({ mutationFn: serversApi.remove });

  let node: ReactNode = null;
  switch (dialog?.kind) {
    case "create":
      node = (
        <ServerFormModal
          server={null}
          onClose={() => closeIf("create")}
          onCreated={(result) =>
            setDialog({
              kind: "command",
              serverName: result.server.name,
              command: result.install_command,
            })
          }
        />
      );
      break;
    case "edit":
      node = <ServerFormModal server={dialog.server} onClose={() => closeIf("edit")} />;
      break;
    case "command":
      node = (
        <InstallCommandModal
          serverName={dialog.serverName}
          command={dialog.command}
          onClose={() => closeIf("command")}
        />
      );
      break;
    case "regenerate": {
      const server = dialog.server;
      node = (
        <ConfirmDialog
          isOpen
          onOpenChange={(open) => !open && closeIf("regenerate")}
          title={`重新生成「${server.name}」的安装命令？`}
          confirmLabel="重新生成"
          status="warning"
          onConfirm={async () => {
            const result = await regenerate.mutateAsync(server.id);
            setDialog({
              kind: "command",
              serverName: server.name,
              command: result.install_command,
            });
          }}
        >
          <p>旧的 Agent Token 立即失效，正在运行的 Agent 会断开。</p>
          <p>生成后要到这台服务器上重新执行新的安装命令（重装或换机器时用）。</p>
        </ConfirmDialog>
      );
      break;
    }
    case "upgrade": {
      const server = dialog.server;
      node = (
        <ConfirmDialog
          isOpen
          onOpenChange={(open) => !open && closeIf("upgrade")}
          title={`升级「${server.name}」的 Agent？`}
          confirmLabel="升级"
          status="accent"
          onConfirm={async () => {
            await upgrade.mutateAsync(server.id);
            toast.success("已发出升级指令", {
              description: "稍后看列表里的 Agent 版本；升级失败会自动回滚。",
            });
          }}
        >
          <p>
            从 {server.agent_version || "当前版本"} 升级到 {version ?? "主控的版本"}
            。升级时 Agent 会重启，节点短暂断开；失败会自动换回旧版本。
          </p>
          {server.online ? null : (
            <p className="text-warning">这台服务器现在离线，升级指令可能发不出去。</p>
          )}
        </ConfirmDialog>
      );
      break;
    }
    case "upgrade-all": {
      const count = dialog.servers.length;
      node = (
        <ConfirmDialog
          isOpen
          onOpenChange={(open) => !open && closeIf("upgrade-all")}
          title="升级全部服务器的 Agent？"
          confirmLabel="全部升级"
          status="accent"
          onConfirm={async () => {
            const result = await upgradeAll.mutateAsync();
            toast.success(`已向 ${result.started} 台服务器发出升级指令`, {
              description: "稍后看列表里的 Agent 版本；失败的原因显示在版本下面。",
            });
          }}
        >
          <p>
            有 {count} 台在线服务器的 Agent 和主控版本（{version ?? "-"}）不一致。
          </p>
          <p>升级时 Agent 会重启，节点短暂断开；失败会自动回滚。</p>
          <p>离线的服务器连上后再单独升级。</p>
        </ConfirmDialog>
      );
      break;
    }
    case "delete": {
      const server = dialog.server;
      node = (
        <ConfirmDialog
          isOpen
          onOpenChange={(open) => !open && closeIf("delete")}
          title={`删除服务器「${server.name}」？`}
          confirmLabel="删除"
          onConfirm={async () => {
            await remove.mutateAsync(server.id);
            toast.success(`已删除服务器「${server.name}」`);
            options.onDeleted?.();
          }}
        >
          <p>这台服务器上的节点和证书会一起删除，用户的订阅里也不再有这些节点。</p>
          <p>Agent 会自动卸载：在线时马上断开，离线的下次连上主控时卸载。</p>
        </ConfirmDialog>
      );
      break;
    }
    default:
      node = null;
  }

  return { open: setDialog, dialog: node, masterVersion: version };
}

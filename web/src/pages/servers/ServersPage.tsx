import { ArrowUpFromLine, Ellipsis, Plus } from "@gravity-ui/icons";
import { Button, Dropdown, Label, Table } from "@heroui/react";
import { useQuery } from "@tanstack/react-query";
import { Link, useNavigate } from "@tanstack/react-router";
import { serversQuery } from "../../api/queries";
import type { Server } from "../../api/types";
import { PageHeader } from "../../components/PageHeader";
import { EmptyHint, QueryView } from "../../components/QueryView";
import { UsageBar } from "../../components/UsageBar";
import {
  AgentVersion,
  canUpgrade,
  ServerStatusChip,
  SpeedCell,
  SyncStatus,
  useServerDialogs,
} from "./serverParts";

/** 服务器列表：每 10 秒刷新一次在线状态、网速、用量和同步情况。 */
export function ServersPage() {
  const servers = useQuery(serversQuery);
  const { open, dialog, masterVersion } = useServerDialogs();
  // 「全部升级」只给在线、版本和主控不同的服务器发指令（hosting.rs 的 upgrade_all）
  const upgradable = (servers.data ?? []).filter((s) => s.online && canUpgrade(s, masterVersion));

  return (
    <>
      <PageHeader
        title="服务器"
        description="装了 Agent 的机器。列表每 10 秒刷新一次。"
        actions={
          <>
            {upgradable.length > 0 ? (
              <Button
                variant="secondary"
                onPress={() => open({ kind: "upgrade-all", servers: upgradable })}
              >
                <ArrowUpFromLine />
                全部升级（{upgradable.length}）
              </Button>
            ) : null}
            <Button onPress={() => open({ kind: "create" })}>
              <Plus />
              添加服务器
            </Button>
          </>
        }
      />
      <QueryView query={servers}>
        {(list) => <ServerTable servers={list} masterVersion={masterVersion} onAction={open} />}
      </QueryView>
      {dialog}
    </>
  );
}

type OpenDialog = ReturnType<typeof useServerDialogs>["open"];

function ServerTable({
  servers,
  masterVersion,
  onAction,
}: {
  servers: Server[];
  masterVersion: string | undefined;
  onAction: OpenDialog;
}) {
  const navigate = useNavigate();
  return (
    <Table>
      <Table.ScrollContainer>
        <Table.Content aria-label="服务器列表" className="min-w-[1080px]">
          <Table.Header>
            <Table.Column isRowHeader>名称</Table.Column>
            <Table.Column>状态</Table.Column>
            <Table.Column>Agent 版本</Table.Column>
            <Table.Column>实时网速</Table.Column>
            <Table.Column className="w-64">本月用量 / 额度</Table.Column>
            <Table.Column>配置同步</Table.Column>
            <Table.Column className="w-14 text-end">操作</Table.Column>
          </Table.Header>
          <Table.Body
            renderEmptyState={() => (
              <EmptyHint>
                还没有服务器。点右上角「添加服务器」，再到服务器上执行安装命令。
              </EmptyHint>
            )}
          >
            {servers.map((server) => (
              <Table.Row key={server.id} id={server.id}>
                <Table.Cell>
                  <div className="flex flex-col">
                    <Link
                      to="/servers/$serverId"
                      params={{ serverId: String(server.id) }}
                      className="font-medium text-foreground hover:text-accent"
                    >
                      {server.name}
                    </Link>
                    <span className="font-mono text-xs text-muted">{server.address}</span>
                  </div>
                </Table.Cell>
                <Table.Cell>
                  <ServerStatusChip server={server} />
                </Table.Cell>
                <Table.Cell>
                  <AgentVersion server={server} masterVersion={masterVersion} />
                </Table.Cell>
                <Table.Cell>
                  <SpeedCell server={server} />
                </Table.Cell>
                <Table.Cell>
                  <UsageBar
                    used={server.month_rx + server.month_tx}
                    quota={server.traffic_quota_bytes}
                    label="本月用量"
                  />
                </Table.Cell>
                <Table.Cell>
                  <SyncStatus server={server} />
                </Table.Cell>
                <Table.Cell className="text-end">
                  <Dropdown>
                    <Button
                      isIconOnly
                      size="sm"
                      variant="ghost"
                      aria-label={`${server.name} 的操作`}
                    >
                      <Ellipsis />
                    </Button>
                    <Dropdown.Popover placement="bottom end">
                      <Dropdown.Menu
                        onAction={(key) => {
                          switch (key) {
                            case "detail":
                              void navigate({
                                to: "/servers/$serverId",
                                params: { serverId: String(server.id) },
                              });
                              break;
                            case "edit":
                              onAction({ kind: "edit", server });
                              break;
                            case "regenerate":
                              onAction({ kind: "regenerate", server });
                              break;
                            case "upgrade":
                              onAction({ kind: "upgrade", server });
                              break;
                            case "delete":
                              onAction({ kind: "delete", server });
                              break;
                          }
                        }}
                      >
                        <Dropdown.Item id="detail" textValue="详情">
                          <Label>详情和网卡流量</Label>
                        </Dropdown.Item>
                        <Dropdown.Item id="edit" textValue="编辑">
                          <Label>编辑</Label>
                        </Dropdown.Item>
                        <Dropdown.Item id="regenerate" textValue="重新生成安装命令">
                          <Label>重新生成安装命令</Label>
                        </Dropdown.Item>
                        <Dropdown.Item id="upgrade" textValue="升级 Agent">
                          <Label>
                            升级 Agent
                            {canUpgrade(server, masterVersion) ? "" : "（已是最新）"}
                          </Label>
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

import { Copy, Ellipsis, Plus } from "@gravity-ui/icons";
import { Button, Dropdown, Label, Separator, Table } from "@heroui/react";
import { useQuery } from "@tanstack/react-query";
import { Link, useNavigate } from "@tanstack/react-router";
import { usersQuery } from "../../api/queries";
import type { User } from "../../api/types";
import { useToday } from "../../components/DateRangeBar";
import { PageHeader } from "../../components/PageHeader";
import { EmptyHint, QueryView } from "../../components/QueryView";
import { UsageBar } from "../../components/UsageBar";
import { ExpiresOn, UserStatus, useUserDialogs } from "./userParts";

/** 用户列表：套餐、本周期用量、到期、状态；停用和恢复要同步到服务器，列表每 10 秒刷新。 */
export function UsersPage() {
  const users = useQuery(usersQuery);
  const actions = useUserDialogs();

  return (
    <>
      <PageHeader
        title="用户"
        description="拼车的朋友。每人一个订阅链接，按套餐的额度每月重置；到期、超额自动停用。"
        actions={
          <Button onPress={() => actions.open({ kind: "create" })}>
            <Plus />
            添加用户
          </Button>
        }
      />
      <QueryView query={users}>{(list) => <UserTable users={list} actions={actions} />}</QueryView>
      {actions.dialog}
    </>
  );
}

function UserTable({
  users,
  actions,
}: {
  users: User[];
  actions: ReturnType<typeof useUserDialogs>;
}) {
  const navigate = useNavigate();
  const today = useToday();
  return (
    <Table>
      <Table.ScrollContainer>
        <Table.Content aria-label="用户列表" className="min-w-[1000px]">
          <Table.Header>
            <Table.Column isRowHeader>名称</Table.Column>
            <Table.Column>套餐</Table.Column>
            <Table.Column className="w-64">本周期已用 / 额度</Table.Column>
            <Table.Column>到期</Table.Column>
            <Table.Column>状态</Table.Column>
            <Table.Column className="w-40 text-end">操作</Table.Column>
          </Table.Header>
          <Table.Body
            renderEmptyState={() => <EmptyHint>还没有用户。先建套餐，再添加用户。</EmptyHint>}
          >
            {users.map((user) => (
              <Table.Row key={user.id} id={user.id}>
                <Table.Cell>
                  <div className="flex flex-col">
                    <Link
                      to="/users/$userId"
                      params={{ userId: String(user.id) }}
                      className="font-medium text-foreground hover:text-accent"
                    >
                      {user.name}
                    </Link>
                    {user.remark ? (
                      <span className="max-w-60 truncate text-xs text-muted">{user.remark}</span>
                    ) : null}
                  </div>
                </Table.Cell>
                <Table.Cell>{user.plan_name}</Table.Cell>
                <Table.Cell>
                  <UsageBar used={user.used_bytes} quota={user.quota_bytes} label="本周期用量" />
                </Table.Cell>
                <Table.Cell>
                  <ExpiresOn user={user} today={today} />
                </Table.Cell>
                <Table.Cell>
                  <UserStatus user={user} />
                </Table.Cell>
                <Table.Cell className="text-end">
                  <div className="flex items-center justify-end gap-1">
                    <Button size="sm" variant="secondary" onPress={() => actions.copyLink(user)}>
                      <Copy className="size-3.5" />
                      订阅链接
                    </Button>
                    <Dropdown>
                      <Button
                        isIconOnly
                        size="sm"
                        variant="ghost"
                        aria-label={`${user.name} 的操作`}
                      >
                        <Ellipsis />
                      </Button>
                      <Dropdown.Popover placement="bottom end">
                        <Dropdown.Menu
                          onAction={(key) => {
                            switch (key) {
                              case "detail":
                                void navigate({
                                  to: "/users/$userId",
                                  params: { userId: String(user.id) },
                                });
                                break;
                              case "edit":
                                actions.open({ kind: "edit", user });
                                break;
                              case "renew":
                                actions.open({ kind: "renew", user });
                                break;
                              case "reset-period":
                                actions.open({ kind: "reset-period", user });
                                break;
                              case "toggle":
                                actions.toggleEnabled(user);
                                break;
                              case "reset-credentials":
                                actions.open({ kind: "reset-credentials", user });
                                break;
                              case "delete":
                                actions.open({ kind: "delete", user });
                                break;
                            }
                          }}
                        >
                          <Dropdown.Item id="detail" textValue="流量明细">
                            <Label>流量明细</Label>
                          </Dropdown.Item>
                          <Dropdown.Item id="edit" textValue="编辑">
                            <Label>编辑</Label>
                          </Dropdown.Item>
                          <Dropdown.Item id="renew" textValue="续期">
                            <Label>续期</Label>
                          </Dropdown.Item>
                          <Dropdown.Item id="reset-period" textValue="清零本周期用量">
                            <Label>清零本周期用量</Label>
                          </Dropdown.Item>
                          <Dropdown.Item id="toggle" textValue={user.enabled ? "停用" : "启用"}>
                            <Label>{user.enabled ? "停用" : "启用"}</Label>
                          </Dropdown.Item>
                          <Separator />
                          <Dropdown.Item
                            id="reset-credentials"
                            textValue="重置订阅链接和凭据"
                            variant="danger"
                          >
                            <Label>重置订阅链接和凭据</Label>
                          </Dropdown.Item>
                          <Dropdown.Item id="delete" textValue="删除" variant="danger">
                            <Label>删除</Label>
                          </Dropdown.Item>
                        </Dropdown.Menu>
                      </Dropdown.Popover>
                    </Dropdown>
                  </div>
                </Table.Cell>
              </Table.Row>
            ))}
          </Table.Body>
        </Table.Content>
      </Table.ScrollContainer>
    </Table>
  );
}

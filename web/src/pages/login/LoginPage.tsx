import { Alert, Card, Form } from "@heroui/react";
import { zodResolver } from "@hookform/resolvers/zod";
import { useMutation } from "@tanstack/react-query";
import { useRouter, useSearch } from "@tanstack/react-router";
import { useForm } from "react-hook-form";
import { z } from "zod";
import { authApi } from "../../api/endpoints";
import { ActionButton } from "../../components/ActionButton";
import { TextInput } from "../../components/form";
import { queryClient } from "../../queryClient";

const schema = z.object({
  username: z.string().trim().min(1, "请填写用户名"),
  password: z.string().min(1, "请填写密码"),
});

type Values = z.infer<typeof schema>;

/** 登录后回到哪里：只接受站内路径，防止被带到别的网站。 */
function safeRedirect(target: string | undefined): string {
  if (target?.startsWith("/") && !target.startsWith("//") && !target.startsWith("/login")) {
    return target;
  }
  return "/";
}

export function LoginPage() {
  const router = useRouter();
  const search = useSearch({ from: "/login" });
  const form = useForm<Values>({
    resolver: zodResolver(schema),
    defaultValues: { username: "", password: "" },
  });
  const login = useMutation({
    mutationFn: authApi.login,
    // 用户名或密码不对也是 401：显示在表单里，不当成登录过期
    meta: { keepCache: true, silent: true, allowUnauthorized: true },
    onSuccess: () => {
      queryClient.clear();
      router.history.push(safeRedirect(search.redirect));
    },
  });

  const submit = form.handleSubmit((v) => login.mutateAsync(v));

  return (
    <div className="flex min-h-screen items-center justify-center bg-background px-4">
      <Card className="w-full max-w-sm p-2">
        <Card.Header>
          <Card.Title className="text-xl">登录 open-proxy</Card.Title>
          <Card.Description>管理面板</Card.Description>
        </Card.Header>
        <Card.Content>
          <Form
            className="flex flex-col gap-4"
            validationBehavior="aria"
            onSubmit={(e) => {
              void submit(e).catch(() => {});
            }}
          >
            <TextInput
              control={form.control}
              name="username"
              label="用户名"
              autoComplete="username"
              autoFocus
            />
            <TextInput
              control={form.control}
              name="password"
              label="密码"
              type="password"
              autoComplete="current-password"
            />
            {login.isError ? (
              <Alert status="danger">
                <Alert.Indicator />
                <Alert.Content>
                  <Alert.Description>{login.error.message}</Alert.Description>
                </Alert.Content>
              </Alert>
            ) : null}
            <ActionButton type="submit" fullWidth isPending={login.isPending}>
              登录
            </ActionButton>
          </Form>
        </Card.Content>
      </Card>
    </div>
  );
}

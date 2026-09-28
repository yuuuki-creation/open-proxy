import { Alert, Card, Form } from "@heroui/react";
import { zodResolver } from "@hookform/resolvers/zod";
import { useMutation } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { useForm } from "react-hook-form";
import { z } from "zod";
import { setupApi } from "../../api/endpoints";
import { ActionButton } from "../../components/ActionButton";
import { TextInput } from "../../components/form";
import { TimeZoneInput } from "../../components/TimeZoneInput";
import { browserTimeZone } from "../../lib/date";
import { domainSchema } from "../../lib/validation";
import { queryClient } from "../../queryClient";

const schema = z
  .object({
    username: z.string().trim().min(1, "请填写用户名").max(64, "最长 64 个字"),
    password: z.string().min(8, "密码至少 8 位"),
    confirm: z.string(),
    domain: domainSchema,
    cloudflare_api_token: z.string().trim(),
    timezone: z.string().min(1, "请选择时区"),
  })
  .refine((v) => v.password === v.confirm, {
    message: "两次输入的密码不一样",
    path: ["confirm"],
  });

type Values = z.infer<typeof schema>;

/** 首次初始化：设置管理员账号、主控域名、Cloudflare Token、时区。谁先打开谁初始化。 */
export function SetupPage() {
  const navigate = useNavigate();
  const form = useForm<Values>({
    resolver: zodResolver(schema),
    defaultValues: {
      username: "admin",
      password: "",
      confirm: "",
      domain: "",
      cloudflare_api_token: "",
      timezone: browserTimeZone(),
    },
  });
  const setup = useMutation({
    mutationFn: setupApi.init,
    // 错误显示在表单下面，不再弹提示
    meta: { keepCache: true, silent: true },
    onSuccess: async () => {
      queryClient.clear();
      await navigate({ to: "/" });
    },
  });

  const submit = form.handleSubmit((v) =>
    setup.mutateAsync({
      username: v.username,
      password: v.password,
      domain: v.domain || undefined,
      cloudflare_api_token: v.cloudflare_api_token || undefined,
      timezone: v.timezone,
    }),
  );

  return (
    <div className="flex min-h-screen items-center justify-center bg-background px-4 py-10">
      <Card className="w-full max-w-lg p-2">
        <Card.Header>
          <Card.Title className="text-xl">初始化 open-proxy</Card.Title>
          <Card.Description>
            第一次使用，先设置管理员账号。谁先打开这个页面谁就能初始化，部署后请尽快完成。
          </Card.Description>
        </Card.Header>
        <Card.Content>
          <Form
            className="flex flex-col gap-4"
            validationBehavior="aria"
            onSubmit={(e) => {
              // 失败时错误显示在表单下面
              submit(e).catch(() => {});
            }}
          >
            <TextInput
              control={form.control}
              name="username"
              label="管理员用户名"
              isRequired
              autoComplete="username"
            />
            <TextInput
              control={form.control}
              name="password"
              label="密码"
              type="password"
              isRequired
              autoComplete="new-password"
              description="至少 8 位"
            />
            <TextInput
              control={form.control}
              name="confirm"
              label="再输入一次密码"
              type="password"
              isRequired
              autoComplete="new-password"
            />
            <TextInput
              control={form.control}
              name="domain"
              label="主控域名"
              placeholder="panel.example.com"
              description="订阅链接和 Agent 安装命令都用这个域名；可以以后在设置里填。"
            />
            <TextInput
              control={form.control}
              name="cloudflare_api_token"
              label="Cloudflare API Token"
              type="password"
              mono
              description="用来自动申请证书（DNS 验证），只保存不显示；可以以后在设置里填。"
            />
            <TimeZoneInput
              control={form.control}
              name="timezone"
              label="时区"
              description="每日流量、每月重置、到期都按这个时区算。"
            />
            {setup.isError ? (
              <Alert status="danger">
                <Alert.Indicator />
                <Alert.Content>
                  <Alert.Description>{setup.error.message}</Alert.Description>
                </Alert.Content>
              </Alert>
            ) : null}
            <ActionButton type="submit" fullWidth isPending={setup.isPending}>
              完成初始化
            </ActionButton>
          </Form>
        </Card.Content>
      </Card>
    </div>
  );
}

import { DatabaseArrowRight, FileArrowUp } from "@gravity-ui/icons";
import {
  Alert,
  Button,
  Chip,
  Form,
  Input,
  Label,
  Modal,
  Spinner,
  TextField,
  toast,
} from "@heroui/react";
import { zodResolver } from "@hookform/resolvers/zod";
import { useMutation, useQuery } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { useEffect, useRef, useState } from "react";
import { useForm } from "react-hook-form";
import { z } from "zod";
import { download } from "../../api/client";
import { authApi, backupApi, settingsApi, setupApi } from "../../api/endpoints";
import { settingsQuery } from "../../api/queries";
import type { Settings } from "../../api/types";
import { ActionButton } from "../../components/ActionButton";
import { ConfirmDialog } from "../../components/ConfirmDialog";
import { TextInput } from "../../components/form";
import { InfoList } from "../../components/InfoList";
import { PageHeader, Section } from "../../components/PageHeader";
import { QueryView } from "../../components/QueryView";
import { TimeZoneInput } from "../../components/TimeZoneInput";
import { todayIn } from "../../lib/date";
import { formatBytes } from "../../lib/format";
import { domainSchema } from "../../lib/validation";
import { queryClient } from "../../queryClient";

/** 设置：主控域名和时区、Cloudflare Token（只写）、管理员密码、数据备份与恢复。 */
export function SettingsPage() {
  const settings = useQuery(settingsQuery);
  return (
    <>
      <PageHeader title="设置" />
      <QueryView query={settings}>
        {(s) => (
          <div className="flex flex-col gap-6">
            <GeneralSettings settings={s} />
            <CloudflareToken settings={s} />
            <ChangePassword />
            <Backup timezone={s.timezone} />
          </div>
        )}
      </QueryView>
    </>
  );
}

const generalSchema = z.object({
  domain: domainSchema,
  timezone: z.string().min(1, "请选择时区"),
});

type GeneralValues = z.infer<typeof generalSchema>;

function GeneralSettings({ settings }: { settings: Settings }) {
  const form = useForm<GeneralValues>({
    resolver: zodResolver(generalSchema),
    defaultValues: { domain: settings.domain, timezone: settings.timezone },
  });
  const save = useMutation({
    mutationFn: settingsApi.update,
    onSuccess: (saved) => {
      toast.success("已保存设置");
      form.reset({ domain: saved.domain, timezone: saved.timezone });
    },
  });
  const submit = form.handleSubmit((v) =>
    save.mutateAsync({ domain: v.domain, timezone: v.timezone }),
  );

  return (
    <Section title="主控">
      <div className="mb-5">
        <InfoList
          items={[
            {
              label: "对外地址",
              value: settings.public_url ? (
                <span className="font-mono">{settings.public_url}</span>
              ) : (
                <span className="text-warning">
                  还没有（填了域名才有），订阅链接和安装命令都要用它
                </span>
              ),
            },
            { label: "主控版本", value: <span className="font-mono">{settings.version}</span> },
          ]}
        />
      </div>
      <Form
        className="flex max-w-xl flex-col gap-4"
        validationBehavior="aria"
        onSubmit={(e) => {
          submit(e).catch(() => {});
        }}
      >
        <TextInput
          control={form.control}
          name="domain"
          label="主控域名"
          placeholder="panel.example.com"
          description="面板、订阅链接和 Agent 安装命令都用这个域名；主控的 HTTPS 证书也按它申请。改了以后旧的订阅链接失效。"
        />
        <TimeZoneInput
          control={form.control}
          name="timezone"
          label="时区"
          description="每日流量、每月重置、到期都按这个时区算。"
        />
        <div>
          <ActionButton type="submit" isPending={save.isPending}>
            保存
          </ActionButton>
        </div>
      </Form>
    </Section>
  );
}

function CloudflareToken({ settings }: { settings: Settings }) {
  const [token, setToken] = useState("");
  const [clearing, setClearing] = useState(false);
  const save = useMutation({
    mutationFn: (value: string) => settingsApi.update({ cloudflare_api_token: value }),
  });

  return (
    <Section
      title="Cloudflare API Token"
      description="主控用它做 DNS 验证，申请主控和服务器的证书。只保存不显示；要换就重新填一个。"
      actions={
        settings.cloudflare_api_token_set ? (
          <Chip size="sm" color="success" variant="soft">
            已设置
          </Chip>
        ) : (
          <Chip size="sm" variant="soft">
            未设置
          </Chip>
        )
      }
    >
      <form
        className="flex max-w-xl items-end gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          if (!token.trim()) {
            return;
          }
          save
            .mutateAsync(token.trim())
            .then(() => {
              setToken("");
              toast.success("已保存 Cloudflare Token");
            })
            .catch(() => {});
        }}
      >
        <TextField className="flex-1" type="password" value={token} onChange={setToken}>
          <Label>{settings.cloudflare_api_token_set ? "换一个新的 Token" : "Token"}</Label>
          <Input className="font-mono" autoComplete="off" placeholder="需要 Zone.DNS 编辑权限" />
        </TextField>
        <ActionButton
          type="submit"
          isPending={save.isPending && !clearing}
          isDisabled={!token.trim()}
        >
          保存 Token
        </ActionButton>
        {settings.cloudflare_api_token_set ? (
          <Button variant="danger-soft" onPress={() => setClearing(true)}>
            清除
          </Button>
        ) : null}
      </form>
      {clearing ? (
        <ConfirmDialog
          isOpen
          onOpenChange={(open) => !open && setClearing(false)}
          title="清除 Cloudflare Token？"
          confirmLabel="清除"
          onConfirm={async () => {
            await save.mutateAsync("");
            toast.success("已清除 Cloudflare Token");
          }}
        >
          <p>清除后主控不能再自动申请和续期证书，已经申请到的证书到期前照常使用。</p>
        </ConfirmDialog>
      ) : null}
    </Section>
  );
}

const passwordSchema = z
  .object({
    old_password: z.string().min(1, "请填写原密码"),
    new_password: z.string().min(8, "新密码至少 8 位"),
    confirm: z.string(),
  })
  .refine((v) => v.new_password === v.confirm, {
    message: "两次输入的新密码不一样",
    path: ["confirm"],
  });

type PasswordValues = z.infer<typeof passwordSchema>;

function ChangePassword() {
  const navigate = useNavigate();
  const form = useForm<PasswordValues>({
    resolver: zodResolver(passwordSchema),
    defaultValues: { old_password: "", new_password: "", confirm: "" },
  });
  const change = useMutation({
    mutationFn: authApi.changePassword,
    meta: { keepCache: true },
    onSuccess: async () => {
      toast.success("密码已修改，请用新密码重新登录", {
        description: "所有登录会话都已失效。",
      });
      queryClient.clear();
      await navigate({ to: "/login" });
    },
  });
  const submit = form.handleSubmit((v) =>
    change.mutateAsync({ old_password: v.old_password, new_password: v.new_password }),
  );

  return (
    <Section title="管理员密码" description="修改后所有登录会话失效，要重新登录。">
      <Form
        className="flex max-w-xl flex-col gap-4"
        validationBehavior="aria"
        onSubmit={(e) => {
          submit(e).catch(() => {});
        }}
      >
        <TextInput
          control={form.control}
          name="old_password"
          label="原密码"
          type="password"
          autoComplete="current-password"
        />
        <div className="grid grid-cols-2 gap-4">
          <TextInput
            control={form.control}
            name="new_password"
            label="新密码"
            type="password"
            autoComplete="new-password"
            description="至少 8 位"
          />
          <TextInput
            control={form.control}
            name="confirm"
            label="再输入一次新密码"
            type="password"
            autoComplete="new-password"
          />
        </div>
        <div>
          <ActionButton type="submit" isPending={change.isPending}>
            修改密码
          </ActionButton>
        </div>
      </Form>
    </Section>
  );
}

function Backup({ timezone }: { timezone: string }) {
  const fileInput = useRef<HTMLInputElement>(null);
  const [confirmDownload, setConfirmDownload] = useState(false);
  const [restoreFile, setRestoreFile] = useState<File | null>(null);
  const [restarting, setRestarting] = useState(false);
  const restore = useMutation({
    mutationFn: backupApi.restore,
    meta: { keepCache: true },
  });

  return (
    <Section
      title="数据备份"
      description="备份是整个数据库（在线备份，不停服务）；恢复会用备份覆盖当前的全部数据。"
    >
      <div className="flex flex-wrap gap-2">
        <Button variant="secondary" onPress={() => setConfirmDownload(true)}>
          <DatabaseArrowRight />
          下载备份
        </Button>
        <input
          ref={fileInput}
          type="file"
          className="hidden"
          accept=".db,.sqlite,.sqlite3,application/octet-stream"
          onChange={(e) => {
            const file = e.target.files?.[0];
            e.target.value = "";
            if (file) {
              setRestoreFile(file);
            }
          }}
        />
        <Button variant="danger-soft" onPress={() => fileInput.current?.click()}>
          <FileArrowUp />
          从备份恢复…
        </Button>
      </div>

      {confirmDownload ? (
        <ConfirmDialog
          isOpen
          onOpenChange={(open) => !open && setConfirmDownload(false)}
          title="下载备份"
          confirmLabel="我知道了，下载"
          status="warning"
          onConfirm={async () => {
            const day = todayIn(timezone).replaceAll("-", "");
            try {
              await download("/api/backup", `op-master-${day}.db`);
            } catch (err) {
              toast.danger(err instanceof Error ? err.message : "下载失败");
              throw err;
            }
          }}
        >
          <p className="font-medium text-warning">备份文件包含全部凭据，请妥善保管。</p>
          <p>
            里面有所有用户的订阅链接和节点凭据、出口密码、Cloudflare
            Token、证书私钥。不要发给别人，也不要放在公开的地方。
          </p>
        </ConfirmDialog>
      ) : null}

      {restoreFile ? (
        <ConfirmDialog
          isOpen
          onOpenChange={(open) => !open && setRestoreFile(null)}
          title="从备份恢复？"
          confirmLabel="恢复"
          onConfirm={async () => {
            await restore.mutateAsync(restoreFile);
            setRestarting(true);
          }}
        >
          <p>
            用「{restoreFile.name}」（{formatBytes(restoreFile.size)}）恢复。
          </p>
          <Alert status="danger">
            <Alert.Indicator />
            <Alert.Content>
              <Alert.Description>
                当前的全部数据（服务器、节点、用户、流量记录、设置）都会被备份里的数据覆盖，不能撤销。恢复后主控自动重启，需要重新登录。
              </Alert.Description>
            </Alert.Content>
          </Alert>
        </ConfirmDialog>
      ) : null}

      {restarting ? <RestartingModal /> : null}
    </Section>
  );
}

/** 恢复备份后主控自动重启：等它重新可以访问，再回登录页。 */
function RestartingModal() {
  const [waited, setWaited] = useState(0);
  useEffect(() => {
    let stopped = false;
    const started = Date.now();
    const poll = async () => {
      // 先等几秒，让主控有时间退出
      await new Promise((resolve) => window.setTimeout(resolve, 3_000));
      while (!stopped) {
        setWaited(Math.round((Date.now() - started) / 1000));
        try {
          await setupApi.status();
          break;
        } catch {
          if (Date.now() - started > 60_000) {
            break;
          }
          await new Promise((resolve) => window.setTimeout(resolve, 2_000));
        }
      }
      if (!stopped) {
        queryClient.clear();
        window.location.assign("/login");
      }
    };
    void poll();
    return () => {
      stopped = true;
    };
  }, []);

  return (
    <Modal.Backdrop isOpen isDismissable={false} isKeyboardDismissDisabled>
      <Modal.Container size="sm">
        <Modal.Dialog aria-label="主控正在重启">
          <Modal.Body className="flex flex-col items-center gap-3 py-8 text-center">
            <Spinner size="lg" />
            <p className="font-medium">已恢复备份，主控正在重启…</p>
            <p className="text-sm text-muted">重启好以后自动回到登录页（已等 {waited} 秒）。</p>
          </Modal.Body>
        </Modal.Dialog>
      </Modal.Container>
    </Modal.Backdrop>
  );
}

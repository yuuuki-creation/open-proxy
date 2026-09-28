import { ArrowRotateRight, FileArrowUp } from "@gravity-ui/icons";
import {
  Alert,
  Button,
  Chip,
  Form,
  Label,
  ListBox,
  Select,
  Spinner,
  Table,
  toast,
} from "@heroui/react";
import { zodResolver } from "@hookform/resolvers/zod";
import { useMutation, useQuery } from "@tanstack/react-query";
import { useRef, useState } from "react";
import { useForm, useWatch } from "react-hook-form";
import { z } from "zod";
import { templateApi } from "../../api/endpoints";
import {
  templatePreviewQuery,
  templateQuery,
  templateReportQuery,
  usersQuery,
} from "../../api/queries";
import type { SubFormat, Template, TemplateReport } from "../../api/types";
import { ActionButton } from "../../components/ActionButton";
import { RadioInput, TextAreaInput, TextInput } from "../../components/form";
import { InfoList } from "../../components/InfoList";
import { PageHeader, Section } from "../../components/PageHeader";
import { EmptyHint, ErrorBlock, LoadingBlock, QueryView } from "../../components/QueryView";
import { copyWithToast } from "../../lib/clipboard";
import { formatDateTime } from "../../lib/format";
import {
  DROPPED_KIND_LABELS,
  FORMAT_LABELS,
  FORMATS,
  TEMPLATE_SOURCE_LABELS,
} from "../../lib/labels";

/** 订阅模板：只维护一份 Mihomo 模板，其他格式由主控翻译；翻译不了的写进兼容性报告。 */
export function TemplatePage() {
  const template = useQuery(templateQuery);
  return (
    <>
      <PageHeader
        title="订阅模板"
        description="只维护一份 Mihomo（Clash）模板，其他客户端的配置由主控翻译。模板里用占位符标记节点插入的位置。"
      />
      <QueryView query={template}>
        {(t) => (
          <div className="flex flex-col gap-6">
            <CurrentTemplate template={t} />
            <TemplateEditor template={t} />
            <ReportSection />
            <PreviewSection />
          </div>
        )}
      </QueryView>
    </>
  );
}

function CurrentTemplate({ template: t }: { template: Template }) {
  const refresh = useMutation({
    mutationFn: templateApi.refresh,
    onSuccess: (result) => {
      if (result.last_error) {
        toast.warning("拉取失败，继续用上一份", { description: result.last_error });
      } else {
        toast.success("已拉取最新的远程模板");
      }
    },
  });
  return (
    <Section
      title="当前模板"
      actions={
        t.source === "remote" ? (
          <ActionButton
            size="sm"
            variant="secondary"
            isPending={refresh.isPending}
            onPress={() => refresh.mutate()}
          >
            <ArrowRotateRight />
            立即拉取
          </ActionButton>
        ) : null
      }
    >
      <InfoList
        items={[
          {
            label: "来源",
            value: (
              <Chip size="sm" variant="soft" color={t.source === "builtin" ? "default" : "accent"}>
                {TEMPLATE_SOURCE_LABELS[t.source]}
              </Chip>
            ),
          },
          ...(t.source === "remote"
            ? [
                {
                  label: "远程地址",
                  value: <span className="break-all font-mono text-xs">{t.remote_url}</span>,
                  wide: true,
                },
                { label: "拉取周期", value: `每 ${t.refresh_hours} 小时` },
                { label: "上次拉取", value: formatDateTime(t.last_fetch_at) },
              ]
            : []),
        ]}
      />
      {t.last_error ? (
        <Alert status="warning" className="mt-4">
          <Alert.Indicator />
          <Alert.Content>
            <Alert.Title>上次拉取失败，正在用上一份模板</Alert.Title>
            <Alert.Description>{t.last_error}</Alert.Description>
          </Alert.Content>
        </Alert>
      ) : null}
    </Section>
  );
}

const schema = z
  .object({
    source: z.enum(["builtin", "custom", "remote"]),
    content: z.string(),
    remote_url: z.string().trim(),
    refresh_hours: z.string().trim(),
  })
  .superRefine((v, ctx) => {
    if (v.source === "custom" && !v.content.trim()) {
      ctx.addIssue({ code: "custom", message: "请粘贴或上传模板正文", path: ["content"] });
    }
    if (v.source === "remote") {
      if (!/^https?:\/\/\S+$/.test(v.remote_url)) {
        ctx.addIssue({
          code: "custom",
          message: "要以 http:// 或 https:// 开头",
          path: ["remote_url"],
        });
      }
      const hours = Number(v.refresh_hours);
      if (!/^\d+$/.test(v.refresh_hours) || hours < 1 || hours > 720) {
        ctx.addIssue({ code: "custom", message: "要在 1–720 小时之间", path: ["refresh_hours"] });
      }
    }
  });

type Values = z.infer<typeof schema>;

function TemplateEditor({ template: t }: { template: Template }) {
  const fileInput = useRef<HTMLInputElement>(null);
  const form = useForm<Values>({
    resolver: zodResolver(schema),
    defaultValues: {
      source: t.source,
      // 从内置或远程改成自定义时，拿当前生效的正文做底稿
      content: t.content,
      remote_url: t.remote_url ?? "",
      refresh_hours: String(t.refresh_hours || 24),
    },
  });
  const source = useWatch({ control: form.control, name: "source" });
  const save = useMutation({
    mutationFn: templateApi.update,
    onSuccess: (saved) => {
      toast.success("已保存订阅模板", {
        description: "新的模板对之后拉取的订阅生效；兼容性报告已更新。",
      });
      form.reset({
        source: saved.source,
        content: saved.content,
        remote_url: saved.remote_url ?? "",
        refresh_hours: String(saved.refresh_hours || 24),
      });
    },
  });

  const submit = form.handleSubmit((v) =>
    save.mutateAsync(
      v.source === "custom"
        ? { source: "custom", content: v.content }
        : v.source === "remote"
          ? { source: "remote", remote_url: v.remote_url, refresh_hours: Number(v.refresh_hours) }
          : { source: "builtin" },
    ),
  );

  const upload = async (file: File | undefined) => {
    if (!file) {
      return;
    }
    if (file.size > 2 * 1024 * 1024) {
      toast.danger("文件太大（超过 2 MB），不像是订阅模板");
      return;
    }
    form.setValue("content", await file.text(), { shouldDirty: true, shouldValidate: true });
    toast.success(`已读入 ${file.name}，检查无误后点保存`);
  };

  return (
    <Section
      title="更换模板"
      description="保存时主控会先检查模板能不能解析；远程地址会先拉取一次，拉不到就不切换。"
    >
      <Form
        className="flex flex-col gap-4"
        validationBehavior="aria"
        onSubmit={(e) => {
          submit(e).catch(() => {});
        }}
      >
        <RadioInput
          control={form.control}
          name="source"
          label="来源"
          orientation="horizontal"
          options={[
            {
              id: "builtin",
              label: "内置模板",
              description: "只有「手动选择」和「自动选最快」两个组，能完整翻译到所有格式。",
            },
            { id: "custom", label: "自定义", description: "粘贴或上传一份 Mihomo 配置。" },
            { id: "remote", label: "远程地址", description: "主控定时拉取；拉取失败保留上一份。" },
          ]}
        />
        {source === "custom" ? (
          <div className="flex flex-col gap-2">
            <div>
              <input
                ref={fileInput}
                type="file"
                accept=".yaml,.yml,.txt,text/yaml,text/plain"
                className="hidden"
                onChange={(e) => {
                  void upload(e.target.files?.[0]);
                  e.target.value = "";
                }}
              />
              <Button size="sm" variant="secondary" onPress={() => fileInput.current?.click()}>
                <FileArrowUp />
                从文件读入
              </Button>
            </div>
            <TextAreaInput
              control={form.control}
              name="content"
              label="模板正文（Mihomo YAML）"
              rows={18}
              mono
            />
          </div>
        ) : null}
        {source === "remote" ? (
          <div className="grid grid-cols-3 gap-4">
            <TextInput
              className="col-span-2"
              control={form.control}
              name="remote_url"
              label="远程地址"
              isRequired
              mono
              placeholder="https://example.com/template.yaml"
            />
            <TextInput
              control={form.control}
              name="refresh_hours"
              label="拉取周期（小时）"
              inputMode="numeric"
              isRequired
            />
          </div>
        ) : null}
        <div>
          <ActionButton type="submit" isPending={save.isPending}>
            保存模板
          </ActionButton>
        </div>
      </Form>
    </Section>
  );
}

function ReportSection() {
  const report = useQuery(templateReportQuery);
  return (
    <Section
      title="兼容性报告"
      description="其他格式由 Mihomo 模板翻译，翻译不了的部分会被丢掉。Mihomo 原样使用，不丢东西。"
    >
      <QueryView query={report}>{(data) => <ReportTable report={data} />}</QueryView>
    </Section>
  );
}

function ReportTable({ report }: { report: TemplateReport }) {
  return (
    <div className="flex flex-col gap-4">
      {report.formats.map(({ format, dropped }) => (
        <div key={format} className="flex flex-col gap-2">
          <div className="flex items-center gap-2">
            <span className="text-sm font-medium text-foreground">
              {FORMAT_LABELS[format] ?? format}
            </span>
            {dropped.length === 0 ? (
              <Chip size="sm" color="success" variant="soft">
                全部保留
              </Chip>
            ) : (
              <Chip size="sm" color="warning" variant="soft">
                丢掉 {dropped.length} 项
              </Chip>
            )}
          </div>
          {dropped.length > 0 ? (
            <Table variant="secondary">
              <Table.ScrollContainer>
                <Table.Content aria-label={`${FORMAT_LABELS[format] ?? format} 丢掉的内容`}>
                  <Table.Header>
                    <Table.Column className="w-24">类型</Table.Column>
                    <Table.Column isRowHeader>内容</Table.Column>
                    <Table.Column>原因</Table.Column>
                  </Table.Header>
                  <Table.Body>
                    {dropped.map((d) => (
                      <Table.Row key={`${d.kind}-${d.item}`} id={`${d.kind}-${d.item}`}>
                        <Table.Cell>{DROPPED_KIND_LABELS[d.kind] ?? d.kind}</Table.Cell>
                        <Table.Cell className="break-all font-mono text-xs">{d.item}</Table.Cell>
                        <Table.Cell className="text-sm">{d.reason}</Table.Cell>
                      </Table.Row>
                    ))}
                  </Table.Body>
                </Table.Content>
              </Table.ScrollContainer>
            </Table>
          ) : null}
        </div>
      ))}
    </div>
  );
}

function PreviewSection() {
  const users = useQuery(usersQuery);
  const [format, setFormat] = useState<SubFormat>("mihomo");
  const [userId, setUserId] = useState<string>("");
  const [requested, setRequested] = useState<{ format: SubFormat; userId: number } | null>(null);
  const preview = useQuery({
    ...templatePreviewQuery(requested?.format ?? "mihomo", requested?.userId ?? 0),
    enabled: requested !== null,
  });
  const list = users.data ?? [];
  const selectedUser = userId || (list[0] ? String(list[0].id) : "");

  return (
    <Section
      title="预览"
      description="按某个用户生成某种格式的订阅内容，看模板和节点展开得对不对。"
    >
      <div className="flex flex-wrap items-end gap-3">
        <Select
          className="w-56"
          value={format}
          onChange={(key) => key !== null && setFormat(String(key) as SubFormat)}
        >
          <Label>格式</Label>
          <Select.Trigger>
            <Select.Value />
            <Select.Indicator />
          </Select.Trigger>
          <Select.Popover>
            <ListBox>
              {FORMATS.map((f) => (
                <ListBox.Item key={f} id={f} textValue={FORMAT_LABELS[f]}>
                  {FORMAT_LABELS[f]}
                  <ListBox.ItemIndicator />
                </ListBox.Item>
              ))}
            </ListBox>
          </Select.Popover>
        </Select>
        <Select
          className="w-56"
          placeholder={list.length === 0 ? "还没有用户" : "选一个用户"}
          isDisabled={list.length === 0}
          value={selectedUser || null}
          onChange={(key) => setUserId(key === null ? "" : String(key))}
        >
          <Label>用户</Label>
          <Select.Trigger>
            <Select.Value />
            <Select.Indicator />
          </Select.Trigger>
          <Select.Popover>
            <ListBox>
              {list.map((u) => (
                <ListBox.Item key={u.id} id={String(u.id)} textValue={u.name}>
                  {u.name}
                  <ListBox.ItemIndicator />
                </ListBox.Item>
              ))}
            </ListBox>
          </Select.Popover>
        </Select>
        <Button
          variant="secondary"
          isDisabled={!selectedUser}
          onPress={() => {
            setRequested({ format, userId: Number(selectedUser) });
            if (requested?.format === format && requested.userId === Number(selectedUser)) {
              void preview.refetch();
            }
          }}
        >
          生成预览
        </Button>
      </div>
      <div className="mt-4">
        {requested === null ? (
          <EmptyHint>选好格式和用户后点「生成预览」。</EmptyHint>
        ) : preview.isFetching && preview.data === undefined ? (
          <LoadingBlock />
        ) : preview.isError ? (
          <ErrorBlock error={preview.error} onRetry={() => void preview.refetch()} />
        ) : preview.data !== undefined ? (
          <div className="flex flex-col gap-2">
            <div className="flex items-center gap-2">
              <Button
                size="sm"
                variant="ghost"
                onPress={() => void copyWithToast(preview.data ?? "", "预览内容")}
              >
                复制全部
              </Button>
              {preview.isFetching ? <Spinner size="sm" /> : null}
            </div>
            <pre className="max-h-[480px] overflow-auto rounded-xl bg-surface-secondary p-4 font-mono text-xs leading-relaxed text-foreground">
              {preview.data}
            </pre>
          </div>
        ) : null}
      </div>
      <p className="mt-3 text-xs text-muted">要把订阅发给朋友，在用户页复制订阅链接。</p>
    </Section>
  );
}

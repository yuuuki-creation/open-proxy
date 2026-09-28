// 界面上显示的名称：协议、订阅格式、停用原因等。

import type {
  ApplyFailure,
  BlockedReason,
  CertMode,
  ExitKind,
  Protocol,
  SubFormat,
  TemplateSource,
} from "../api/types";

export interface ProtocolInfo {
  id: Protocol;
  label: string;
  description: string;
}

export const PROTOCOLS: ProtocolInfo[] = [
  {
    id: "vless_reality",
    label: "VLESS + REALITY",
    description: "TCP，伪装成访问别的网站的 TLS 流量，抗封锁能力强。需要选一个伪装目标。",
  },
  {
    id: "hysteria2",
    label: "Hysteria2",
    description: "基于 QUIC（UDP），弱网下速度好。可以开端口跳跃和 salamander 混淆。",
  },
  {
    id: "anytls",
    label: "AnyTLS",
    description: "TCP + TLS，各客户端支持得最全。用这台服务器的证书。",
  },
  {
    id: "shadowsocks2022",
    label: "Shadowsocks 2022",
    description: "加密方式固定为 2022-blake3-aes-128-gcm，简单、兼容性好。",
  },
  {
    id: "mieru",
    label: "Mieru",
    description: "TCP，只有 Mihomo 系客户端和分享链接能用。",
  },
];

export function protocolLabel(protocol: string): string {
  return PROTOCOLS.find((p) => p.id === protocol)?.label ?? protocol;
}

export const FORMATS: SubFormat[] = [
  "mihomo",
  "stash",
  "shadowrocket",
  "surge",
  "loon",
  "quantumultx",
  "v2ray",
];

export const FORMAT_LABELS: Record<SubFormat, string> = {
  mihomo: "Mihomo（Clash）",
  stash: "Stash",
  shadowrocket: "Shadowrocket",
  surge: "Surge",
  loon: "Loon",
  quantumultx: "Quantumult X",
  v2ray: "v2rayN / v2rayNG",
};

export function formatLabel(format: string): string {
  return FORMAT_LABELS[format as SubFormat] ?? format;
}

/**
 * 创建节点前预估「在哪些客户端里看不到」，和主控 subscription/node.rs 的能力表一致；
 * 节点建好后以列表里主控给的 hidden_in 为准。
 */
export function predictHiddenIn(protocol: Protocol, obfs: boolean): SubFormat[] {
  switch (protocol) {
    case "vless_reality":
      return ["surge"];
    case "hysteria2":
      return obfs ? ["surge", "quantumultx"] : ["quantumultx"];
    case "mieru":
      return ["stash", "shadowrocket", "surge", "loon", "quantumultx"];
    default:
      return [];
  }
}

export const BLOCKED_REASON_LABELS: Record<Exclude<BlockedReason, "">, string> = {
  manual: "手动停用",
  expired: "已到期",
  over_quota: "超额",
};

export function blockedReasonLabel(reason: string): string {
  return BLOCKED_REASON_LABELS[reason as Exclude<BlockedReason, "">] ?? reason;
}

export const APPLY_ITEM_LABELS: Record<ApplyFailure["item"], string> = {
  node: "节点",
  exit: "落地出口",
  landing: "落地入站",
  certificate: "证书",
  port_hopping: "端口跳跃",
  unknown: "其他",
};

export const CERT_MODE_LABELS: Record<CertMode, string> = {
  acme: "自动申请（Cloudflare DNS 验证）",
  self_signed: "自签证书",
};

export const EXIT_KIND_LABELS: Record<ExitKind, string> = {
  third_party: "第三方 SOCKS5",
  self_built: "自建落地机",
};

export const TEMPLATE_SOURCE_LABELS: Record<TemplateSource, string> = {
  builtin: "内置模板",
  custom: "自定义（粘贴或上传）",
  remote: "远程地址",
};

export const DROPPED_KIND_LABELS: Record<string, string> = {
  all: "全部",
  section: "配置段",
  proxy_group: "代理组",
  rule: "规则",
};

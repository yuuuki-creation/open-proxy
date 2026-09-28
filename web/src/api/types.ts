// 主控 API 的请求和响应类型，和 master/src/api/*.rs 里的结构一一对应（不生成 OpenAPI，改接口时两边一起改）。
// 约定：字段 snake_case；ID 是整数；时间是 RFC 3339（UTC）；日期是 YYYY-MM-DD（管理员时区）；流量是字节。

/** 错误响应：`code` 给前端判断，`message` 直接显示。 */
export interface ApiErrorBody {
  code: string;
  message: string;
}

// ---------- 初始化、登录、会话（auth.rs） ----------

export interface SetupStatus {
  initialized: boolean;
}

export interface SetupRequest {
  username: string;
  password: string;
  domain?: string;
  cloudflare_api_token?: string;
  timezone?: string;
}

export interface LoginRequest {
  username: string;
  password: string;
}

export interface AdminInfo {
  username: string;
}

export interface PasswordRequest {
  old_password: string;
  new_password: string;
}

// ---------- 设置（settings.rs） ----------

export interface Settings {
  domain: string;
  timezone: string;
  /** Cloudflare Token 只写，读取时只返回是否已设置 */
  cloudflare_api_token_set: boolean;
  /** 对外地址（拼安装命令和订阅链接用）；还没有域名时为 null */
  public_url: string | null;
  /** 主控版本；Agent 版本和它不同就可以升级 */
  version: string;
}

export interface UpdateSettings {
  domain?: string;
  timezone?: string;
  /** 空字符串表示清除 */
  cloudflare_api_token?: string;
}

// ---------- 概览和流量统计（stats.rs） ----------

export interface UpDown {
  up: number;
  down: number;
}

export interface DayPoint {
  day: string;
  up: number;
  down: number;
}

export interface TopUser {
  id: number;
  name: string;
  used_bytes: number;
  quota_bytes: number | null;
}

export interface Overview {
  today: UpDown;
  /** 本自然月 */
  month: UpDown;
  /** 最近 30 天，没有流量的日子也补 0 */
  daily: DayPoint[];
  /** 本周期用量排行，最多 10 个 */
  top_users: TopUser[];
  servers: { total: number; online: number };
  users: { total: number; blocked: number };
}

export type TrafficGroupBy = "user" | "node" | "server";

export interface TrafficSeries {
  id: number;
  /** 已删除的显示为「已删除（#id）」 */
  name: string;
  /** 只有有流量的日子 */
  points: DayPoint[];
}

export interface TrafficStats {
  from: string;
  to: string;
  series: TrafficSeries[];
}

export interface UserTrafficRow {
  day: string;
  node_id: number;
  node_name: string;
  up: number;
  down: number;
}

export interface UserTraffic {
  from: string;
  to: string;
  rows: UserTrafficRow[];
}

export interface ServerTrafficRow {
  day: string;
  rx: number;
  tx: number;
}

export interface ServerTraffic {
  from: string;
  to: string;
  rows: ServerTrafficRow[];
}

// ---------- 服务器（servers.rs） ----------

export type CertMode = "acme" | "self_signed";

/** 应用失败的项（state/mod.rs 的 record_report） */
export interface ApplyFailure {
  item: "node" | "exit" | "landing" | "certificate" | "port_hopping" | "unknown";
  /** 节点 ID 或出口 ID；落地和证书为 0 */
  id: number;
  reason: string;
}

export interface Certificate {
  kind: CertMode;
  domain: string | null;
  sha256: string;
  not_after: string;
  renewed_at: string;
  last_error: string;
}

export interface Server {
  id: number;
  name: string;
  address: string;
  port_range_start: number;
  port_range_end: number;
  cert_mode: CertMode;
  cert_domain: string | null;
  traffic_quota_bytes: number | null;
  traffic_reset_day: number | null;
  online: boolean;
  /** 在线但和主控版本不一致：按本地状态继续服务，暂停同步，等升级 */
  version_mismatch: boolean;
  /** 实时网速（字节/秒） */
  rx_speed: number;
  tx_speed: number;
  /** 从上一个重置日起的网卡收发 */
  month_rx: number;
  month_tx: number;
  agent_version: string;
  agent_arch: string;
  /** 上次升级失败、换回旧版本时要升级到的版本；空表示没有 */
  rolled_back_from: string;
  /** 最近一次升级指令失败的原因（只在主控内存里） */
  upgrade_error: string | null;
  last_seen_at: string | null;
  state_version: number;
  applied_version: number;
  /** Agent 已经应用了最新的期望状态 */
  synced: boolean;
  apply_failures: ApplyFailure[];
  certificate: Certificate | null;
  created_at: string;
}

export interface ServerFields {
  name: string;
  address: string;
  port_range_start?: number;
  port_range_end?: number;
  cert_mode: CertMode;
  cert_domain?: string | null;
  traffic_quota_bytes?: number | null;
  traffic_reset_day?: number | null;
}

export type UpdateServer = Partial<ServerFields>;

export interface CreateServerResponse {
  server: Server;
  /** Agent Token 只在这里出现一次 */
  install_command: string;
}

export interface InstallCommand {
  install_command: string;
}

export interface UpgradeAllResponse {
  started: number;
}

// ---------- REALITY 伪装目标（P6，接口按约定先写） ----------

export interface RealityCheckResult {
  target: string;
  tls13: boolean;
  h2: boolean;
  latency_ms: number;
  certificate_valid: boolean;
  /** 空表示没有错误 */
  error: string;
}

export interface RealityCheckResponse {
  results: RealityCheckResult[];
}

export interface RealityScanRequest {
  cidr?: string;
  concurrency?: number;
  max_per_second?: number;
}

export type RealityScanStatus = "idle" | "running" | "done" | "failed";

export interface RealityCandidate {
  ip: string;
  domain: string;
  issuer: string;
  latency_ms: number;
}

export interface RealityScan {
  status: RealityScanStatus;
  started_at: string | null;
  finished_at: string | null;
  error: string;
  candidates: RealityCandidate[];
}

// ---------- 节点（nodes.rs） ----------

export type Protocol = "vless_reality" | "hysteria2" | "anytls" | "shadowsocks2022" | "mieru";

export interface PortRange {
  start: number;
  end: number;
}

export interface RealityInfo {
  public_key: string;
  short_id: string;
  /** host:port */
  target: string;
}

/** 订阅格式（subscription/node.rs 的 Format::name） */
export type SubFormat =
  | "mihomo"
  | "stash"
  | "shadowrocket"
  | "surge"
  | "loon"
  | "quantumultx"
  | "v2ray";

export interface Node {
  id: number;
  server_id: number;
  server_name: string;
  name: string;
  protocol: Protocol;
  port: number;
  hop_ports: PortRange | null;
  /** 实际使用的地址：节点单独设置的，或者服务器的 */
  address: string;
  /** 节点单独设置的地址 */
  address_override: string | null;
  exit_id: number | null;
  exit_name: string | null;
  enabled: boolean;
  sort_order: number;
  reality: RealityInfo | null;
  /** Hysteria2 是否开了 salamander 混淆 */
  obfs: boolean;
  /** 在哪些订阅格式里看不到（P4 起才有） */
  hidden_in?: SubFormat[];
  created_at: string;
}

export interface CreateNode {
  server_id: number;
  name: string;
  protocol: Protocol;
  /** 不填就在服务器的端口范围里自动分配 */
  port?: number;
  address?: string;
  exit_id?: number | null;
  enabled?: boolean;
  /** VLESS + REALITY 必填：host 或 host:port */
  reality_target?: string;
  obfs?: boolean;
  hop_ports?: PortRange | null;
}

export interface UpdateNode {
  name?: string;
  port?: number;
  /** null 表示改回用服务器的地址 */
  address?: string | null;
  /** null 表示直连 */
  exit_id?: number | null;
  enabled?: boolean;
  reality_target?: string;
  obfs?: boolean;
  /** null 表示关闭端口跳跃 */
  hop_ports?: PortRange | null;
}

// ---------- 落地出口（exits.rs） ----------

export type ExitKind = "third_party" | "self_built";

export interface Exit {
  id: number;
  name: string;
  kind: ExitKind;
  /** 第三方的是登记的地址，自建的是落地机的地址 */
  host: string;
  port: number;
  username: string;
  password: string;
  landing_server_id: number | null;
  landing_server_name: string | null;
  /** 有多少个节点在用 */
  node_count: number;
  created_at: string;
}

export interface CreateExit {
  name: string;
  kind: ExitKind;
  host?: string;
  port?: number;
  username?: string;
  password?: string;
  landing_server_id?: number;
}

export interface UpdateExit {
  name?: string;
  host?: string;
  port?: number;
  username?: string;
  password?: string;
}

// ---------- 套餐（plans.rs） ----------

export interface Plan {
  id: number;
  name: string;
  /** 每个周期的流量额度，null 表示不限 */
  traffic_quota_bytes: number | null;
  node_ids: number[];
  user_count: number;
  created_at: string;
}

export interface PlanFields {
  name: string;
  traffic_quota_bytes: number | null;
  node_ids: number[];
}

// ---------- 用户（users.rs） ----------

export type BlockedReason = "" | "manual" | "expired" | "over_quota";

export interface User {
  id: number;
  name: string;
  remark: string;
  plan_id: number;
  plan_name: string;
  /** 套餐的流量额度，null 表示不限 */
  quota_bytes: number | null;
  enabled: boolean;
  started_on: string;
  /** null 表示永久 */
  expires_on: string | null;
  /** 本周期用量 */
  used_bytes: number;
  up_total: number;
  down_total: number;
  blocked_reason: BlockedReason;
  blocked_since: string | null;
  /** 主控还没有域名时为 null */
  sub_url: string | null;
  /** 这个用户涉及的服务器里，还没同步最新期望状态的台数 */
  pending_servers: number;
  created_at: string;
}

export interface CreateUser {
  name: string;
  remark?: string;
  plan_id: number;
  started_on?: string;
  expires_on?: string | null;
  enabled?: boolean;
}

export interface UpdateUser {
  name?: string;
  remark?: string;
  plan_id?: number;
  enabled?: boolean;
  started_on?: string;
  /** null 表示永久 */
  expires_on?: string | null;
}

// ---------- 订阅模板（P4，template.rs） ----------

export type TemplateSource = "builtin" | "custom" | "remote";

export interface Template {
  source: TemplateSource;
  /** 当前生效的正文（内置时是内置模板） */
  content: string;
  remote_url: string | null;
  refresh_hours: number;
  last_fetch_at: string | null;
  last_error: string;
}

export interface UpdateTemplate {
  source: TemplateSource;
  content?: string;
  remote_url?: string;
  refresh_hours?: number;
}

export interface DroppedItem {
  /** section / proxy_group / rule / all 等 */
  kind: string;
  item: string;
  reason: string;
}

export interface TemplateReport {
  formats: { format: SubFormat; dropped: DroppedItem[] }[];
}

// ---------- 备份（P7） ----------

export interface RestoreResponse {
  restarting: boolean;
}

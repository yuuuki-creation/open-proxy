// 每个接口一个函数。路径和请求体见 api.md 和 master/src/api/*.rs；
// 标了「P4 / P6 / P7」的接口主控还在实现，按约定先写好。

import { api, withQuery } from "./client";
import type {
  AdminInfo,
  CreateExit,
  CreateNode,
  CreateServerResponse,
  CreateUser,
  Exit,
  InstallCommand,
  LoginRequest,
  Node,
  Overview,
  PasswordRequest,
  Plan,
  PlanFields,
  RealityCheckResponse,
  RealityScan,
  RealityScanRequest,
  RestoreResponse,
  Server,
  ServerFields,
  ServerTraffic,
  Settings,
  SetupRequest,
  SetupStatus,
  SubFormat,
  Template,
  TemplateReport,
  TrafficGroupBy,
  TrafficStats,
  UpdateExit,
  UpdateNode,
  UpdateServer,
  UpdateSettings,
  UpdateTemplate,
  UpdateUser,
  UpgradeAllResponse,
  User,
  UserTraffic,
} from "./types";

type Empty = Record<string, never>;

export interface DateRange {
  from?: string;
  to?: string;
}

export const setupApi = {
  status: (signal?: AbortSignal) => api.get<SetupStatus>("/api/setup", signal),
  init: (body: SetupRequest) => api.post<AdminInfo>("/api/setup", body),
};

export const authApi = {
  login: (body: LoginRequest) => api.post<AdminInfo>("/api/auth/login", body),
  logout: () => api.post<Empty>("/api/auth/logout"),
  me: (signal?: AbortSignal) => api.get<AdminInfo>("/api/auth/me", signal),
  changePassword: (body: PasswordRequest) => api.put<Empty>("/api/auth/password", body),
};

export const settingsApi = {
  get: (signal?: AbortSignal) => api.get<Settings>("/api/settings", signal),
  update: (body: UpdateSettings) => api.patch<Settings>("/api/settings", body),
};

export const statsApi = {
  overview: (signal?: AbortSignal) => api.get<Overview>("/api/overview", signal),
  traffic: (groupBy: TrafficGroupBy, range: DateRange, signal?: AbortSignal) =>
    api.get<TrafficStats>(withQuery("/api/traffic", { group_by: groupBy, ...range }), signal),
  user: (id: number, range: DateRange, signal?: AbortSignal) =>
    api.get<UserTraffic>(withQuery(`/api/users/${id}/traffic`, { ...range }), signal),
  server: (id: number, range: DateRange, signal?: AbortSignal) =>
    api.get<ServerTraffic>(withQuery(`/api/servers/${id}/traffic`, { ...range }), signal),
};

export const serversApi = {
  list: (signal?: AbortSignal) => api.get<Server[]>("/api/servers", signal),
  get: (id: number, signal?: AbortSignal) => api.get<Server>(`/api/servers/${id}`, signal),
  create: (body: ServerFields) => api.post<CreateServerResponse>("/api/servers", body),
  update: (id: number, body: UpdateServer) => api.patch<Server>(`/api/servers/${id}`, body),
  remove: (id: number) => api.delete<Empty>(`/api/servers/${id}`),
  /** 重新生成安装命令：换新 Token，旧 Token 立即失效 */
  installCommand: (id: number) => api.post<InstallCommand>(`/api/servers/${id}/install-command`),
  /** P6：发出升级指令就返回，结果看列表里的 Agent 版本和 rolled_back_from */
  upgrade: (id: number) => api.post<Empty>(`/api/servers/${id}/upgrade`),
  upgradeAll: () => api.post<UpgradeAllResponse>("/api/servers/upgrade-all"),
  /** P6：检测伪装目标，等 Agent 回复（最多 1 分钟） */
  realityCheck: (id: number, targets: string[]) =>
    api.post<RealityCheckResponse>(`/api/servers/${id}/reality/check`, { targets }),
  /** P6：发起扫描（同一台服务器同时只跑一个） */
  realityScan: (id: number, body: RealityScanRequest) =>
    api.post<{ status: string }>(`/api/servers/${id}/reality/scan`, body),
  realityScanStatus: (id: number, signal?: AbortSignal) =>
    api.get<RealityScan>(`/api/servers/${id}/reality/scan`, signal),
};

export const nodesApi = {
  list: (signal?: AbortSignal) => api.get<Node[]>("/api/nodes", signal),
  create: (body: CreateNode) => api.post<Node>("/api/nodes", body),
  update: (id: number, body: UpdateNode) => api.patch<Node>(`/api/nodes/${id}`, body),
  remove: (id: number) => api.delete<Empty>(`/api/nodes/${id}`),
  /** 拖拽后提交整个顺序 */
  setOrder: (ids: number[]) => api.put<Empty>("/api/nodes/order", { ids }),
};

export const exitsApi = {
  list: (signal?: AbortSignal) => api.get<Exit[]>("/api/exits", signal),
  create: (body: CreateExit) => api.post<Exit>("/api/exits", body),
  update: (id: number, body: UpdateExit) => api.patch<Exit>(`/api/exits/${id}`, body),
  remove: (id: number) => api.delete<Empty>(`/api/exits/${id}`),
};

export const plansApi = {
  list: (signal?: AbortSignal) => api.get<Plan[]>("/api/plans", signal),
  create: (body: PlanFields) => api.post<Plan>("/api/plans", body),
  update: (id: number, body: Partial<PlanFields>) => api.patch<Plan>(`/api/plans/${id}`, body),
  remove: (id: number) => api.delete<Empty>(`/api/plans/${id}`),
};

export const usersApi = {
  list: (signal?: AbortSignal) => api.get<User[]>("/api/users", signal),
  get: (id: number, signal?: AbortSignal) => api.get<User>(`/api/users/${id}`, signal),
  create: (body: CreateUser) => api.post<User>("/api/users", body),
  update: (id: number, body: UpdateUser) => api.patch<User>(`/api/users/${id}`, body),
  remove: (id: number) => api.delete<Empty>(`/api/users/${id}`),
  /** 清零本周期用量 */
  resetPeriod: (id: number) => api.post<User>(`/api/users/${id}/reset-period`),
  /** 同时换订阅链接和全部凭据 */
  resetCredentials: (id: number) => api.post<User>(`/api/users/${id}/reset-credentials`),
};

export const templateApi = {
  get: (signal?: AbortSignal) => api.get<Template>("/api/template", signal),
  update: (body: UpdateTemplate) => api.put<Template>("/api/template", body),
  refresh: () => api.post<Template>("/api/template/refresh"),
  report: (signal?: AbortSignal) => api.get<TemplateReport>("/api/template/report", signal),
  /** 纯文本：按某个用户生成某种格式的订阅正文 */
  preview: (format: SubFormat, userId: number, signal?: AbortSignal) =>
    api.getText(withQuery("/api/template/preview", { format, user_id: userId }), signal),
};

export const backupApi = {
  /** P7：请求体是备份文件的原始字节；之后主控自动重启 */
  restore: (file: File) =>
    api.upload<RestoreResponse>("/api/backup/restore", file, "application/octet-stream"),
};

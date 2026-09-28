// 请求封装：同源 Cookie 认证（op_session）；写请求一律带 Content-Type: application/json，
// 没有请求体的 POST / DELETE 也发 {}（主控对写操作只接受 JSON）。错误统一转成 ApiError。

import type { ApiErrorBody } from "./types";

/** 主控返回的错误：HTTP 状态码加 `{code, message}`，`message` 是中文，直接显示。 */
export class ApiError extends Error {
  readonly status: number;
  readonly code: string;

  constructor(status: number, code: string, message: string) {
    super(message);
    this.name = "ApiError";
    this.status = status;
    this.code = code;
  }
}

/** 没登录或登录已过期。 */
export function isUnauthorized(err: unknown): boolean {
  return err instanceof ApiError && err.status === 401;
}

/** 把任意错误转成能直接显示的中文。 */
export function errorMessage(err: unknown): string {
  if (err instanceof ApiError) {
    return err.message;
  }
  if (err instanceof Error && err.message) {
    return err.message;
  }
  return "出错了，请稍后再试";
}

type Method = "GET" | "POST" | "PUT" | "PATCH" | "DELETE";

interface RequestOptions {
  /** JSON 请求体；写请求不传时发 `{}` */
  body?: unknown;
  /** 原始请求体（恢复备份时上传文件字节），配合 contentType */
  raw?: BodyInit;
  contentType?: string;
  signal?: AbortSignal;
  /** 响应按纯文本读取（订阅预览） */
  text?: boolean;
}

async function send(method: Method, path: string, options: RequestOptions): Promise<Response> {
  const headers: Record<string, string> = { Accept: "application/json" };
  let body: BodyInit | undefined;
  if (method !== "GET") {
    if (options.raw !== undefined) {
      body = options.raw;
      headers["Content-Type"] = options.contentType ?? "application/octet-stream";
    } else {
      body = JSON.stringify(options.body ?? {});
      headers["Content-Type"] = "application/json";
    }
  }
  let res: Response;
  try {
    res = await fetch(path, {
      method,
      headers,
      body,
      credentials: "same-origin",
      signal: options.signal,
    });
  } catch (err) {
    if (err instanceof DOMException && err.name === "AbortError") {
      throw err;
    }
    throw new ApiError(0, "network", "连不上主控，请检查网络后再试");
  }
  if (!res.ok) {
    throw await toApiError(res);
  }
  return res;
}

async function toApiError(res: Response): Promise<ApiError> {
  try {
    const data = (await res.json()) as Partial<ApiErrorBody> | null;
    if (data && typeof data.message === "string" && data.message) {
      return new ApiError(res.status, data.code ?? "error", data.message);
    }
  } catch {
    // 响应不是 JSON（例如反向代理返回的错误页），下面按状态码给一句话
  }
  if (res.status === 401) {
    return new ApiError(401, "unauthorized", "没有登录或登录已过期");
  }
  if (res.status === 502 || res.status === 503 || res.status === 504) {
    return new ApiError(res.status, "unavailable", "主控暂时不可用，请稍后再试");
  }
  return new ApiError(res.status, "http_error", `请求失败（HTTP ${res.status}）`);
}

async function request<T>(method: Method, path: string, options: RequestOptions = {}): Promise<T> {
  const res = await send(method, path, options);
  const text = await res.text();
  if (options.text) {
    return text as T;
  }
  if (!text) {
    return {} as T;
  }
  try {
    return JSON.parse(text) as T;
  } catch {
    throw new ApiError(res.status, "bad_response", "主控返回的内容看不懂（不是 JSON）");
  }
}

export const api = {
  get: <T>(path: string, signal?: AbortSignal) => request<T>("GET", path, { signal }),
  getText: (path: string, signal?: AbortSignal) =>
    request<string>("GET", path, { signal, text: true }),
  post: <T>(path: string, body?: unknown) => request<T>("POST", path, { body }),
  put: <T>(path: string, body?: unknown) => request<T>("PUT", path, { body }),
  patch: <T>(path: string, body?: unknown) => request<T>("PATCH", path, { body }),
  delete: <T>(path: string) => request<T>("DELETE", path),
  /** 上传原始字节（恢复备份）。 */
  upload: <T>(path: string, raw: BodyInit, contentType: string) =>
    request<T>("POST", path, { raw, contentType }),
};

/** 拼查询参数，跳过 undefined 和空字符串。 */
export function withQuery(
  path: string,
  params: Record<string, string | number | undefined | null>,
): string {
  const search = new URLSearchParams();
  for (const [key, value] of Object.entries(params)) {
    if (value !== undefined && value !== null && value !== "") {
      search.set(key, String(value));
    }
  }
  const query = search.toString();
  return query ? `${path}?${query}` : path;
}

/** 从 Content-Disposition 里取文件名。 */
function filenameFrom(header: string | null): string | null {
  if (!header) {
    return null;
  }
  const utf8 = /filename\*=UTF-8''([^;]+)/i.exec(header);
  if (utf8?.[1]) {
    try {
      return decodeURIComponent(utf8[1]);
    } catch {
      // 编码不对就用下面的普通文件名
    }
  }
  const plain = /filename="?([^";]+)"?/i.exec(header);
  return plain?.[1] ?? null;
}

/** 下载文件（带 Cookie），出错时抛 ApiError，不会把错误页当成文件存下来。 */
export async function download(path: string, fallbackName: string): Promise<void> {
  const res = await send("GET", path, {});
  const blob = await res.blob();
  const name = filenameFrom(res.headers.get("Content-Disposition")) ?? fallbackName;
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = name;
  document.body.append(link);
  link.click();
  link.remove();
  window.setTimeout(() => URL.revokeObjectURL(url), 10_000);
}

// 表单校验规则（zod）。和主控的校验一致（master/src/api/mod.rs），提前在浏览器里提示；
// 最终以主控返回的错误为准。表单里的数字都按字符串编辑，提交时再转换。

import { z } from "zod";

/** 是不是 IPv4 地址。 */
export function isIpv4(value: string): boolean {
  const parts = value.split(".");
  return (
    parts.length === 4 &&
    parts.every((p) => /^\d{1,3}$/.test(p) && Number(p) <= 255 && String(Number(p)) === p)
  );
}

/** 是不是域名（字母、数字、连字符，至少两段）。 */
export function isHostname(value: string): boolean {
  if (!value || value.length > 253 || isIpv4(value) || value.includes(":")) {
    return false;
  }
  const labels = value.replace(/\.$/, "").split(".");
  return (
    labels.length >= 2 &&
    labels.every(
      (l) =>
        l.length > 0 &&
        l.length <= 63 &&
        !l.startsWith("-") &&
        !l.endsWith("-") &&
        /^[a-zA-Z0-9-]+$/.test(l),
    )
  );
}

/** 名字：去掉首尾空白后不能为空，最长 64 个字。 */
export function nameSchema(what: string) {
  return z.string().trim().min(1, `请填写${what}`).max(64, `${what}最长 64 个字`);
}

/** 可以不填的域名。 */
export const domainSchema = z
  .string()
  .trim()
  .toLowerCase()
  .refine((v) => v === "" || isHostname(v), "域名格式不对，例如 panel.example.com");

/** 地址：IPv4 或域名（不支持 IPv6）。 */
export const addressSchema = z
  .string()
  .trim()
  .min(1, "请填写地址")
  .refine((v) => isIpv4(v) || isHostname(v), "要写 IPv4 地址或域名（不支持 IPv6）");

/** 可以不填的地址。 */
export const optionalAddressSchema = z
  .string()
  .trim()
  .refine((v) => v === "" || isIpv4(v) || isHostname(v), "要写 IPv4 地址或域名（不支持 IPv6）");

function isPort(v: string): boolean {
  return /^\d+$/.test(v) && Number(v) >= 1 && Number(v) <= 65535;
}

/** 端口：1–65535。 */
export const portSchema = z
  .string()
  .trim()
  .refine((v) => isPort(v), "端口要在 1–65535 之间");

/** 可以不填的端口（不填时自动分配）。 */
export const optionalPortSchema = z
  .string()
  .trim()
  .refine((v) => v === "" || isPort(v), "端口要在 1–65535 之间");

/** 可以不填的流量额度（GB，可以带小数）。 */
export const optionalGbSchema = z
  .string()
  .trim()
  .refine(
    (v) => v === "" || (/^\d+(\.\d+)?$/.test(v) && Number(v) >= 0),
    "要写不小于 0 的数字，单位 GB",
  );

/** 可以不填的整数范围（例如重置日 1–31）。 */
export function optionalIntSchema(min: number, max: number, message: string) {
  return z
    .string()
    .trim()
    .refine((v) => v === "" || (/^\d+$/.test(v) && Number(v) >= min && Number(v) <= max), message);
}

/** 字符串转数字；空字符串是 undefined。 */
export function toNumber(value: string): number | undefined {
  const v = value.trim();
  return v === "" ? undefined : Number(v);
}

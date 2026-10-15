export interface TotpConfig {
  secret: Uint8Array;
  algorithm: "SHA-1" | "SHA-256" | "SHA-512";
  digits: 6 | 8;
  period: number;
  label?: string;
}
function decodeBase32(value: string): Uint8Array {
  const normalized = value.replace(/\s/g, "").toUpperCase();
  if (!/^[A-Z2-7]+={0,6}$/.test(normalized))
    throw new Error("请输入有效的 Base32 密钥（字母 A–Z 和数字 2–7）");
  const raw = normalized.replace(/=+$/, "");
  if (![0, 2, 4, 5, 7].includes(raw.length % 8))
    throw new Error("Base32 密钥长度不正确");
  const alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
  const bytes: number[] = [];
  let bits = 0,
    buffer = 0;
  for (const character of raw) {
    buffer = (buffer << 5) | alphabet.indexOf(character);
    bits += 5;
    if (bits >= 8) {
      bits -= 8;
      bytes.push((buffer >>> bits) & 255);
    }
    buffer &= (1 << bits) - 1;
  }
  if (buffer !== 0 || bytes.length === 0)
    throw new Error("Base32 密钥末尾编码不正确");
  if (normalized.includes("=") && normalized.length % 8 !== 0)
    throw new Error("Base32 密钥填充不正确");
  return new Uint8Array(bytes);
}
export function parseTotp(input: string): TotpConfig {
  const value = input.trim();
  if (!value) throw new Error("请输入 2FA 密钥或 otpauth 链接");
  if (value.length > 8192) throw new Error("输入内容过长");
  let secret = value,
    algorithm: TotpConfig["algorithm"] = "SHA-1",
    digits: 6 | 8 = 6,
    period = 30,
    label: string | undefined;
  if (/^otpauth:/i.test(value)) {
    let url: URL;
    try {
      url = new URL(value);
    } catch {
      throw new Error("otpauth 链接格式不正确");
    }
    if (url.protocol !== "otpauth:" || url.hostname !== "totp")
      throw new Error("仅支持基于时间的 TOTP 链接");
    secret = url.searchParams.get("secret") ?? "";
    const hash = (url.searchParams.get("algorithm") ?? "SHA1").toUpperCase();
    const algorithms: Record<string, TotpConfig["algorithm"]> = {
      SHA1: "SHA-1",
      SHA256: "SHA-256",
      SHA512: "SHA-512",
    };
    if (!algorithms[hash]) throw new Error("仅支持 SHA1、SHA256 或 SHA512");
    algorithm = algorithms[hash];
    const digitValue = url.searchParams.get("digits") ?? "6";
    if (digitValue !== "6" && digitValue !== "8")
      throw new Error("验证码位数必须为 6 或 8");
    digits = Number(digitValue) as 6 | 8;
    const periodValue = url.searchParams.get("period") ?? "30";
    if (
      !/^\d+$/.test(periodValue) ||
      Number(periodValue) < 1 ||
      Number(periodValue) > 86400
    )
      throw new Error("刷新周期必须为 1–86400 秒");
    period = Number(periodValue);
    try {
      label = decodeURIComponent(url.pathname.slice(1));
    } catch {
      throw new Error("账户名称编码不正确");
    }
  }
  return { secret: decodeBase32(secret), algorithm, digits, period, label };
}
export function totpTiming(period: number, now = Date.now()) {
  const seconds = now / 1000;
  const remaining = period - (seconds % period);
  return {
    counter: Math.floor(seconds / period),
    remaining,
    progress: remaining / period,
  };
}
export async function generateTotp(
  config: TotpConfig,
  now = Date.now(),
): Promise<string> {
  if (!Number.isFinite(now) || now < 0) throw new Error("系统时间无效");
  const counter = Math.floor(now / 1000 / config.period);
  const message = new ArrayBuffer(8);
  const view = new DataView(message);
  view.setUint32(0, Math.floor(counter / 4294967296));
  view.setUint32(4, counter >>> 0);
  const key = await crypto.subtle.importKey(
    "raw",
    new Uint8Array(config.secret),
    { name: "HMAC", hash: config.algorithm },
    false,
    ["sign"],
  );
  const signature = new Uint8Array(
    await crypto.subtle.sign("HMAC", key, message),
  );
  const offset = signature[signature.length - 1] & 15;
  const binary =
    ((signature[offset] & 127) << 24) |
    (signature[offset + 1] << 16) |
    (signature[offset + 2] << 8) |
    signature[offset + 3];
  return String(binary % 10 ** config.digits).padStart(config.digits, "0");
}

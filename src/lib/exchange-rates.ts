export interface ExchangeRateSnapshot {
  base: string;
  rates: Record<string, number>;
  updatedAt: string;
  nextUpdateAt: string;
  fetchedAt: string;
  provider: string;
}

export interface ExchangePreferences {
  base: string;
  amount: string;
  targets: string[];
}

export const CURRENCY_CODES = [
  'CNY', 'USD', 'EUR', 'HKD', 'JPY', 'GBP', 'AUD', 'CAD', 'SGD', 'KRW',
  'CHF', 'NZD', 'TWD', 'THB', 'MYR', 'IDR', 'INR', 'AED', 'MOP', 'VND',
] as const;

export const DEFAULT_EXCHANGE_PREFERENCES: ExchangePreferences = {
  base: 'CNY', amount: '100', targets: ['USD', 'EUR', 'HKD', 'JPY'],
};

const isCurrencyCode = (value: unknown): value is string =>
  typeof value === 'string' && /^[A-Z]{3}$/.test(value);

export function getCurrencyName(code: string): string {
  try {
    return new Intl.DisplayNames(['zh-Hans'], { type: 'currency' }).of(code) || code;
  } catch {
    return code;
  }
}

export function getCurrencyOptions(snapshot?: ExchangeRateSnapshot | null): { value: string; label: string }[] {
  const codes = new Set<string>(CURRENCY_CODES);
  Object.keys(snapshot?.rates ?? {}).filter(isCurrencyCode).sort().forEach((code) => codes.add(code));
  if (isCurrencyCode(snapshot?.base)) codes.add(snapshot.base);
  return [...codes].map((value) => ({ value, label: `${getCurrencyName(value)} (${value})` }));
}

export function parseExchangeAmount(input: string): number | null {
  if (!/^\d+(?:\.\d{1,8})?$/.test(input)) return null;
  const value = Number(input);
  return Number.isFinite(value) && value <= 1e12 ? value : null;
}

export function convertCurrency(amount: number, rate: number): number | null {
  if (!Number.isFinite(amount) || amount < 0 || !Number.isFinite(rate) || rate <= 0) return null;
  const result = amount * rate;
  return Number.isFinite(result) ? result : null;
}

export function formatCurrencyAmount(value: number, code: string): string {
  if (!Number.isFinite(value) || value < 0) return '—';
  let digits = 2;
  try {
    digits = new Intl.NumberFormat('zh-CN', { style: 'currency', currency: code })
      .resolvedOptions().maximumFractionDigits ?? 2;
  } catch { /* 未知币种沿用两位小数。 */ }
  // 非零金额低于显示精度时明确标明边界，避免被误读为零。
  const minimum = 10 ** -digits;
  const formatter = new Intl.NumberFormat('zh-CN', { minimumFractionDigits: digits, maximumFractionDigits: digits });
  if (value > 0 && value < minimum) return `< ${formatter.format(minimum)}`;
  return formatter.format(value);
}

export function formatExchangeRate(rate: number): string {
  if (!Number.isFinite(rate) || rate <= 0) return '—';
  return new Intl.NumberFormat('zh-CN', { maximumSignificantDigits: 10 }).format(rate);
}

function defaultPreferences(): ExchangePreferences {
  return { ...DEFAULT_EXCHANGE_PREFERENCES, targets: [...DEFAULT_EXCHANGE_PREFERENCES.targets] };
}

function validatePreferences(value: unknown): ExchangePreferences | null {
  if (!value || typeof value !== 'object') return null;
  const data = value as Partial<ExchangePreferences>;
  if (!isCurrencyCode(data.base) || typeof data.amount !== 'string' || parseExchangeAmount(data.amount) === null
    || !Array.isArray(data.targets) || !data.targets.every(isCurrencyCode)) return null;
  return { base: data.base, amount: data.amount, targets: [...new Set(data.targets)].filter((code) => code !== data.base).slice(0, 30) };
}

export function readExchangePreferences(key: string): ExchangePreferences {
  try {
    const stored = localStorage.getItem(key);
    return stored ? validatePreferences(JSON.parse(stored)) ?? defaultPreferences() : defaultPreferences();
  } catch {
    return defaultPreferences();
  }
}

export function saveExchangePreferences(key: string, value: ExchangePreferences): void {
  const preferences = validatePreferences(value);
  if (!preferences) return;
  try {
    localStorage.setItem(key, JSON.stringify(preferences));
  } catch { /* 存储不可用时保留当前会话输入。 */ }
}

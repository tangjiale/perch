import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  convertCurrency, DEFAULT_EXCHANGE_PREFERENCES, formatCurrencyAmount, formatExchangeRate,
  getCurrencyName, getCurrencyOptions, parseExchangeAmount, readExchangePreferences, saveExchangePreferences,
} from './exchange-rates';

afterEach(() => vi.unstubAllGlobals());

describe('汇率换算', () => {
  it('输入金额可同时按不同币种汇率换算，包括零金额', () => {
    expect(convertCurrency(parseExchangeAmount('100')!, 0.14)).toBeCloseTo(14);
    expect(convertCurrency(100, 20.5)).toBe(2050);
    expect(convertCurrency(0, 0.14)).toBe(0);
  });

  it('限制十进制格式、精度与金额上限', () => {
    expect(parseExchangeAmount('0')).toBe(0);
    expect(parseExchangeAmount('0.00000001')).toBe(1e-8);
    expect(parseExchangeAmount('1000000000000')).toBe(1e12);
    for (const input of ['', '-1', '1e2', '1,000', ' 100 ', '1.', '.5', '0.000000001', '1000000000000.1', 'Infinity']) {
      expect(parseExchangeAmount(input), input).toBeNull();
    }
  });

  it('拒绝无效汇率、金额和溢出结果', () => {
    for (const [amount, rate] of [[-1, 1], [1, 0], [1, -1], [NaN, 1], [1, Infinity], [Number.MAX_VALUE, 2]]) {
      expect(convertCurrency(amount, rate)).toBeNull();
    }
  });

  it('按币种精度显示，并让极小非零值可识别', () => {
    expect(formatCurrencyAmount(1234.567, 'CNY')).toBe('1,234.57');
    expect(formatCurrencyAmount(1234.567, 'JPY')).toBe('1,235');
    expect(formatCurrencyAmount(0, 'CNY')).toBe('0.00');
    expect(formatCurrencyAmount(0.001, 'CNY')).toBe('< 0.01');
    expect(formatCurrencyAmount(0.2, 'JPY')).toBe('< 1');
    expect(formatExchangeRate(0.000000000123)).toBe('0.000000000123');
  });

  it('币种选项合并接口币种、去重并保留常用币种优先', () => {
    const options = getCurrencyOptions({ base: 'CNY', rates: { USD: 0.14, ZAR: 2.5, malformed: 1 }, updatedAt: '', nextUpdateAt: '', fetchedAt: '', provider: '' });
    expect(options[0].value).toBe('CNY');
    expect(options.filter(({ value }) => value === 'USD')).toHaveLength(1);
    expect(options.at(-1)?.value).toBe('ZAR');
    expect(getCurrencyName('CNY')).toContain('人民币');
    expect(getCurrencyName('bad-code')).toBe('bad-code');
  });
});

describe('汇率偏好持久化', () => {
  function storage() {
    const data = new Map<string, string>();
    vi.stubGlobal('localStorage', { getItem: (key: string) => data.get(key) ?? null, setItem: (key: string, value: string) => data.set(key, value) });
    return data;
  }

  it('工作空间键隔离，去除重复和源币种并保存零金额', () => {
    storage();
    saveExchangePreferences('workspace-a', { base: 'CNY', amount: '0', targets: ['CNY', 'USD', 'USD', 'EUR'] });
    expect(readExchangePreferences('workspace-a')).toEqual({ base: 'CNY', amount: '0', targets: ['USD', 'EUR'] });
    expect(readExchangePreferences('workspace-b')).toEqual(DEFAULT_EXCHANGE_PREFERENCES);
  });

  it('损坏 JSON、币种或金额回退默认，默认数组不共享', () => {
    const data = storage();
    for (const value of ['{', 'null', JSON.stringify({ base: 'cny', amount: '100', targets: [] }), JSON.stringify({ base: 'CNY', amount: '-1', targets: ['USD'] })]) {
      data.set('a', value);
      expect(readExchangePreferences('a')).toEqual(DEFAULT_EXCHANGE_PREFERENCES);
    }
    readExchangePreferences('missing').targets.push('GBP');
    expect(readExchangePreferences('missing').targets).not.toContain('GBP');
  });

  it('无效输入不覆盖已存储值，存储故障不抛错', () => {
    storage();
    saveExchangePreferences('a', { base: 'CNY', amount: '50', targets: [] });
    saveExchangePreferences('a', { base: 'CNY', amount: '', targets: [] });
    expect(readExchangePreferences('a').amount).toBe('50');
    vi.stubGlobal('localStorage', { getItem: () => { throw new Error('unavailable'); }, setItem: () => { throw new Error('unavailable'); } });
    expect(readExchangePreferences('a')).toEqual(DEFAULT_EXCHANGE_PREFERENCES);
    expect(() => saveExchangePreferences('a', DEFAULT_EXCHANGE_PREFERENCES)).not.toThrow();
  });
});

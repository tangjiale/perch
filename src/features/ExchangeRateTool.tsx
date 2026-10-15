import { useCallback, useEffect, useRef, useState } from "react";
import { ExternalLink, Plus, RefreshCw, Trash2 } from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import GlassSelect from "../components/GlassSelect";
import { api, native } from "../lib/api";
import type { StorageInfo } from "../lib/types";
import {
  convertCurrency, formatCurrencyAmount, formatExchangeRate,
  getCurrencyName, getCurrencyOptions, parseExchangeAmount, readExchangePreferences,
  saveExchangePreferences, type ExchangeRateSnapshot,
} from "../lib/exchange-rates";
import "./exchange-rates.css";

// 公共汇率在页面间复用；金额与目标币种变化仅做本机计算。
const snapshots = new Map<string, ExchangeRateSnapshot>();
const providerUrl = "https://www.exchangerate-api.com";
const formatTime = (value: string) => new Date(value).toLocaleString("zh-CN", { hour12: false });

export default function ExchangeRateTool({ info, notify }: {
  info: StorageInfo;
  notify: (text: string) => void;
}) {
  const preferenceKey = `perch.exchange:${JSON.stringify([info.workspaceId, info.dataRoot])}`;
  const [preferences, setPreferences] = useState(() => readExchangePreferences(preferenceKey));
  const { base, amount, targets } = preferences;
  const [snapshot, setSnapshot] = useState<ExchangeRateSnapshot | null>(() => snapshots.get(base) ?? null);
  const [loadingBase, setLoadingBase] = useState<string | null>(null);
  const [failure, setFailure] = useState<{ base: string; message: string } | null>(null);
  const generation = useRef(0);

  const loadRates = useCallback(async (requestedBase: string, force = false) => {
    const requestId = ++generation.current;
    const cached = snapshots.get(requestedBase);
    setSnapshot(cached ?? null);
    setFailure(null);
    if (!native) return;
    if (!force && cached && Date.parse(cached.nextUpdateAt) > Date.now()) {
      setLoadingBase(null);
      return;
    }
    setLoadingBase(requestedBase);
    try {
      const result = await api.exchangeRates(requestedBase);
      // 原生接口也校验 base；这里防止迟到响应污染当前选择。
      if (result.base !== requestedBase) throw new Error("返回的来源币种不匹配，请重新刷新");
      if (requestId === generation.current) {
        snapshots.set(requestedBase, result);
        setSnapshot(result);
      }
    } catch (error) {
      if (requestId === generation.current) setFailure({
        base: requestedBase,
        message: error instanceof Error ? error.message : String(error),
      });
    } finally {
      if (requestId === generation.current) setLoadingBase(null);
    }
  }, []);

  useEffect(() => {
    void loadRates(base);
    return () => { generation.current += 1; };
  }, [base, loadRates]);
  useEffect(() => {
    saveExchangePreferences(preferenceKey, preferences);
  }, [preferenceKey, preferences]);

  const current = snapshot?.base === base ? snapshot : null;
  const loading = loadingBase === base;
  const error = failure?.base === base ? failure.message : "";
  const value = parseExchangeAmount(amount);
  const invalidAmount = amount.trim() !== "" && value === null;
  const options = getCurrencyOptions(snapshot);
  // 即使网络暂不可用，也保留上次选中的非热门币种名称。
  for (const code of [base, ...targets]) {
    if (!options.some(option => option.value === code)) options.push({ value: code, label: `${getCurrencyName(code)} (${code})` });
  }
  const available = options.filter(option => option.value !== base && !targets.includes(option.value));
  const overdue = current && Date.parse(current.nextUpdateAt) <= Date.now();

  function changeBase(next: string) {
    setPreferences(previous => ({
      ...previous,
      base: next,
      targets: previous.targets.map(code => code === next ? previous.base : code),
    }));
  }

  return <div className="exchange-tool">
    <div className="tool-panel exchange-input-panel">
      <div className="exchange-intro">
        <h2>换算金额</h2>
        <button onClick={() => void loadRates(base, true)} disabled={!native || loading}>
          <RefreshCw size={15} className={loading ? "exchange-refreshing" : ""} aria-hidden="true" />
          {loading ? "正在查询…" : "刷新汇率"}
        </button>
      </div>
      <div className="exchange-source">
        <label className="tool-field" htmlFor="exchange-amount">金额
          <input id="exchange-amount" type="text" inputMode="decimal" autoComplete="off"
            value={amount} placeholder="例如 100" maxLength={24}
            aria-invalid={invalidAmount} aria-describedby="exchange-amount-hint"
            onChange={event => setPreferences(previous => ({ ...previous, amount: event.target.value }))} />
        </label>
        <div className="tool-field">
          <label htmlFor="exchange-base">来源币种</label>
          <GlassSelect id="exchange-base" aria-label="来源币种" searchable searchPlaceholder="搜索币种名称或代码"
            options={options} value={base} onValueChange={changeBase} />
        </div>
      </div>
      <p id="exchange-amount-hint" className={`tool-hint${invalidAmount ? " exchange-error" : ""}`}>
        {invalidAmount ? "请输入 0 至 1 万亿的有效金额，最多 8 位小数，不含逗号或其他符号。" : "支持小数，修改金额后自动换算。"}
      </p>
    </div>

    {!native && <p className="exchange-notice" role="status">请在栖点桌面应用中查询最新汇率。</p>}
    {error && <p className="exchange-notice exchange-error" role="alert">
      查询失败：{error}。{current ? "仍使用上次获取的汇率，可点击“刷新汇率”重试。" : "请检查网络后点击“刷新汇率”重试。"}
    </p>}

    <div className="tool-panel exchange-results">
      <div className="tool-section-heading">
        <h2>换算结果 <span className="exchange-count">{targets.length} 个币种</span></h2>
        <button disabled={!available.length || targets.length >= 30}
          onClick={() => { if (available[0]) setPreferences(previous => ({ ...previous, targets: [...previous.targets, available[0].value] })); }}>
          <Plus size={15} aria-hidden="true" />添加币种
        </button>
      </div>
      {targets.length === 0 && <p className="exchange-empty">还没有目标币种，点击“添加币种”开始换算。</p>}
      <div className="exchange-rows">
        {targets.map((code, index) => {
          const rate = current?.rates[code];
          const converted = value !== null && rate !== undefined ? convertCurrency(value, rate) : null;
          const rowOptions = options.filter(option => option.value !== base && (option.value === code || !targets.includes(option.value)));
          return <div className="exchange-row" key={code} data-currency={code}>
            <GlassSelect aria-label={`目标币种 ${index + 1}`} searchable searchPlaceholder="搜索币种名称或代码" options={rowOptions} value={code}
              onValueChange={next => setPreferences(previous => ({ ...previous, targets: previous.targets.map(target => target === code ? next : target) }))} />
            <div className="exchange-value">
              <output aria-label={`${code} 换算金额`}>{converted === null ? "—" : formatCurrencyAmount(converted, code)}</output>
              <span className="exchange-currency-code">{code}</span>
              <small>{rate !== undefined
                ? `1 ${base} = ${formatExchangeRate(rate)} ${code}`
                : loading ? "正在查询汇率…" : current ? "该币种暂无汇率" : "等待查询汇率"}</small>
            </div>
            <button className="icon-button exchange-remove" aria-label={`移除 ${code}`} title={`移除 ${code}`}
              onClick={() => setPreferences(previous => ({ ...previous, targets: previous.targets.filter(target => target !== code) }))}>
              <Trash2 size={16} />
            </button>
          </div>;
        })}
      </div>
      {targets.length >= 30 && <p className="tool-hint">已添加 30 个币种，可更换或移除已有币种。</p>}
      <div className="exchange-meta" role="status">
        {current ? <>
          <span>汇率更新于 {formatTime(current.updatedAt)}</span>
          <span>本次获取于 {formatTime(current.fetchedAt)}</span>
          {overdue && <span className="exchange-error">已到下一次更新时段，请刷新汇率。</span>}
          {loading && <span>正在获取最新汇率，当前仍显示上次结果。</span>}
        </> : <span>{loading ? "正在查询最新参考汇率…" : "尚未获取汇率"}</span>}
      </div>
    </div>
    <footer className="exchange-footer">
      <a href={providerUrl} target="_blank" rel="noopener noreferrer"
        onClick={event => {
          if (!native) return;
          event.preventDefault();
          void openUrl(providerUrl).catch(() => notify("打开汇率来源网站失败，请稍后重试"));
        }}>汇率来源：ExchangeRate-API <ExternalLink size={12} aria-hidden="true" /></a>
      <p>参考汇率每日更新，非实时交易报价；实际兑换以银行或支付机构报价为准。</p>
    </footer>
  </div>;
}

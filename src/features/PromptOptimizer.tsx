import { useEffect, useRef, useState } from "react";
import { Link } from "react-router-dom";
import { BookOpen, Check, Copy, FlaskConical, LoaderCircle, RotateCcw, Save, Square, WandSparkles } from "lucide-react";
import GlassSelect from "../components/GlassSelect";
import { api, native } from "../lib/api";
import type { Model, Provider } from "../lib/types";
import { PROMPT_LIMIT, promptKindLabels, promptModels, promptStrategies, type PromptEntry, type PromptKind, type PromptRequest, type PromptResult, type PromptStrategy } from "../lib/prompt-optimizer";
import PromptLibrary, { PromptEditor } from "./PromptLibrary";
import "./prompt-optimizer.css";

const errorText = (error: unknown) => error instanceof Error ? error.message : String(error);
type Comparison = { content: string; error?: string; result?: PromptResult };
type DraftVersion = { id: string; content: string; label: string; result?: PromptResult };

function RunMeta({ result }: { result?: PromptResult }) {
  if (!result) return null;
  const usage = result.usage?.totalTokens != null ? `${result.usage.totalTokens.toLocaleString()} tokens` :
    [result.usage?.inputTokens != null ? `输入 ${result.usage.inputTokens.toLocaleString()}` : "", result.usage?.outputTokens != null ? `输出 ${result.usage.outputTokens.toLocaleString()}` : ""].filter(Boolean).join(" / ") || "用量未提供";
  return <span className="prompt-run-meta">{(result.durationMs / 1000).toFixed(1)} 秒
    {` · ${usage}`}
  </span>;
}

export default function PromptOptimizer({ notify }: { notify: (text: string) => void }) {
  const [tab, setTab] = useState<"optimize" | "library">("optimize");
  const [entries, setEntries] = useState<PromptEntry[]>([]);
  const [loading, setLoading] = useState(native);
  const [libraryError, setLibraryError] = useState("");
  const [models, setModels] = useState<Model[]>([]);
  const [providers, setProviders] = useState<Provider[]>([]);
  const [modelError, setModelError] = useState("");
  const [modelId, setModelId] = useState("");
  const [testModelId, setTestModelId] = useState("");
  const [kind, setKind] = useState<PromptKind>("system");
  const [strategy, setStrategy] = useState<PromptStrategy>("structured");
  const [source, setSource] = useState("");
  const [instruction, setInstruction] = useState("");
  const [output, setOutput] = useState("");
  const [partial, setPartial] = useState("");
  const [result, setResult] = useState<PromptResult>();
  const [history, setHistory] = useState<DraftVersion[]>([]);
  const [versionId, setVersionId] = useState("");
  const [busy, setBusy] = useState<PromptRequest["action"] | null>(null);
  const [stopping, setStopping] = useState(false);
  const [error, setError] = useState("");
  const [testInput, setTestInput] = useState("");
  const [comparison, setComparison] = useState<[Comparison, Comparison]>([{ content: "" }, { content: "" }]);
  const [hasComparison, setHasComparison] = useState(false);
  const [saving, setSaving] = useState(false);
  const [sourceId, setSourceId] = useState<string>();
  const epoch = useRef(0);
  const mounted = useRef(false);
  const requests = useRef(new Set<string>());
  const stoppingRef = useRef(false);
  const loadVersion = useRef(0);
  const libraryMutation = useRef(0);

  const available = promptModels(models, providers);
  const options = available.map((model) => ({ value: model.id, label: `${model.name} · ${providers.find((provider) => provider.id === model.providerId)?.name ?? ""}` }));
  const chosenModel = available.some((model) => model.id === modelId) ? modelId : available[0]?.id ?? "";
  const chosenTestModel = available.some((model) => model.id === testModelId) ? testModelId : chosenModel;
  const sourceEntry = entries.find((entry) => entry.id === sourceId);

  async function reload() {
    if (!native) return;
    const current = ++loadVersion.current;
    const mutation = libraryMutation.current;
    setLoading(true);
    const [library, snapshot] = await Promise.allSettled([api.promptList(), api.snapshot()]);
    if (!mounted.current || current !== loadVersion.current) return;
    // 读取期间若发生写入，保留写入回执；旧列表不能撤销刚保存或删除的条目。
    if (mutation === libraryMutation.current) {
      if (library.status === "fulfilled") { setEntries(library.value); setLibraryError(""); }
      else setLibraryError(errorText(library.reason));
    }
    if (snapshot.status === "fulfilled") { setModels(snapshot.value.models); setProviders(snapshot.value.providers); setModelError(""); }
    else setModelError(errorText(snapshot.reason));
    setLoading(false);
  }

  useEffect(() => {
    mounted.current = true;
    void reload();
    return () => {
      mounted.current = false;
      epoch.current++;
      loadVersion.current++;
      for (const id of requests.current) void api.promptCancel(id).catch(() => {});
    };
  }, []);

  function upsert(entry: PromptEntry) {
    libraryMutation.current++;
    setLibraryError("");
    setEntries((items) => [entry, ...items.filter((item) => item.id !== entry.id)]);
  }
  function removeEntry(id: string) {
    libraryMutation.current++;
    setLibraryError("");
    setEntries((items) => items.filter((item) => item.id !== id));
  }
  function clearComparison() {
    setHasComparison(false);
    setComparison([{ content: "" }, { content: "" }]);
  }
  async function copy(value: string) {
    try { await navigator.clipboard.writeText(value); notify("提示词已复制"); }
    catch { notify("复制失败，请选中文本后手动复制"); }
  }
  async function run(request: Omit<PromptRequest, "generationId">, current: number, onContent: (content: string) => void) {
    const generationId = crypto.randomUUID();
    requests.current.add(generationId);
    try {
      return await api.promptGenerate({ ...request, generationId }, (event) => {
        if (mounted.current && epoch.current === current && event.generationId === generationId) onContent(event.content);
      });
    } finally { requests.current.delete(generationId); }
  }
  async function optimize(action: "optimize" | "iterate") {
    if (busy) return;
    const prompt = action === "iterate" ? output : source;
    if (!prompt.trim() || !chosenModel || (action === "iterate" && !instruction.trim())) return;
    const current = ++epoch.current;
    setBusy(action); setPartial(""); setError(""); clearComparison();
    try {
      const next = await run({ modelId: chosenModel, action, kind, strategy, prompt, instruction }, current, setPartial);
      if (!mounted.current || epoch.current !== current) return;
      setOutput(next.content); setResult(next);
      const id = crypto.randomUUID();
      setHistory((items) => [...items, { id, content: next.content, label: `${action === "iterate" ? "迭代" : "优化"} · ${new Date(next.finishedAt).toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit", second: "2-digit" })}`, result: next }].slice(-20));
      setVersionId(id);
    } catch (failure) {
      if (mounted.current && epoch.current === current) setError(`${errorText(failure)}。已保留原文和上次完成的结果。`);
    } finally {
      if (mounted.current && epoch.current === current) { setBusy(null); setPartial(""); }
    }
  }
  async function cancel() {
    if (stoppingRef.current) return;
    stoppingRef.current = true;
    setStopping(true);
    // 先隔离迟到的流事件；等待原生确认取消后才允许下一次生成。
    epoch.current++;
    const cancelled = await Promise.allSettled([...requests.current].map((id) => api.promptCancel(id)));
    if (!mounted.current) return;
    if (cancelled.some((item) => item.status === "rejected")) {
      setError("停止请求未被确认，请等待当前请求结束后重试。");
      while (requests.current.size && mounted.current) await new Promise((resolve) => window.setTimeout(resolve, 100));
    } else setError("已停止生成，保留上次完成的结果。");
    stoppingRef.current = false;
    if (mounted.current) { setBusy(null); setPartial(""); setStopping(false); }
  }
  async function compare() {
    if (busy || !source.trim() || !output.trim() || !chosenTestModel || (kind === "system" && !testInput.trim())) return;
    const current = ++epoch.current;
    setBusy("test"); setError(""); setHasComparison(true);
    setComparison([{ content: "" }, { content: "" }]);
    await Promise.allSettled([source, output].map(async (prompt, index) => {
      function update(value: Comparison) {
        if (!mounted.current || current !== epoch.current) return;
        setComparison((items) => index === 0 ? [value, items[1]] : [items[0], value]);
      }
      try {
        const response = await run({ modelId: chosenTestModel, action: "test", kind, strategy, prompt, testInput }, current, (content) => update({ content }));
        update({ content: response.content, result: response });
      } catch (failure) {
        if (!mounted.current || current !== epoch.current) return;
        setComparison((items) => {
          const next: [Comparison, Comparison] = [...items];
          next[index] = { ...next[index], error: errorText(failure) };
          return next;
        });
      }
    }));
    if (mounted.current && current === epoch.current) setBusy(null);
  }
  function usePrompt(entry: PromptEntry) {
    if (busy) { notify("请先停止当前生成，再载入提示词"); return; }
    if ((source.trim() || output.trim()) && !window.confirm("载入此提示词将替换当前未保存的工作台内容，是否继续？")) return;
    setSource(entry.content); setKind(entry.kind); setSourceId(entry.id); setOutput(""); setResult(undefined);
    setHistory([]); setVersionId(""); setInstruction(""); clearComparison(); setError(""); setTab("optimize");
  }

  return <div className="prompt-tool">
    <div className="prompt-navigation" aria-label="提示词工具视图">
      <button className={tab === "optimize" ? "active" : ""} aria-pressed={tab === "optimize"} onClick={() => setTab("optimize")}><WandSparkles size={16} />优化工作台</button>
      <button className={tab === "library" ? "active" : ""} aria-pressed={tab === "library"} onClick={() => setTab("library")}><BookOpen size={16} />我的提示词 <span>{entries.length}</span></button>
    </div>
    {!native && <p className="prompt-notice">请在栖点桌面应用中使用模型优化和提示词库，浏览器预览不保存数据。</p>}
    <div hidden={tab !== "optimize"}>
      {modelError && <p role="alert" className="prompt-error">模型配置读取失败：{modelError} <button onClick={() => void reload()}>重试</button></p>}
      {native && !loading && !modelError && !available.length && <p className="prompt-notice">先在 <Link to="/settings?tab=models">模型管理</Link> 中启用一个聊天模型及其供应商，再开始优化。</p>}
      <div className="prompt-settings">
        <label>提示词类型<GlassSelect aria-label="提示词类型" disabled={!!busy} value={kind} options={Object.entries(promptKindLabels).map(([value, label]) => ({ value, label }))} onValueChange={(value) => { setKind(value as PromptKind); clearComparison(); }} /></label>
        <label>优化策略<GlassSelect aria-label="优化策略" disabled={!!busy} value={strategy} options={[...promptStrategies]} onValueChange={(value) => setStrategy(value as PromptStrategy)} /></label>
        <label className="prompt-model">优化模型<GlassSelect aria-label="优化模型" disabled={!!busy || !options.length} value={chosenModel} options={options} placeholder={loading ? "正在读取模型…" : "暂无可用模型"} onValueChange={setModelId} /></label>
      </div>
      <p className="prompt-strategy-hint">{kind === "system" ? "定义 AI 的角色、行为与回答规则。" : "优化一次具体请求的表达与要求。"} {promptStrategies.find((item) => item.value === strategy)?.hint}。</p>
      <div className="prompt-editors">
        <section className="prompt-pane">
          <header><label htmlFor="prompt-source">原始提示词</label><span>{source.length.toLocaleString()} / 30,000</span></header>
          <textarea id="prompt-source" value={source} maxLength={PROMPT_LIMIT} disabled={!!busy} placeholder="例如：你是一位写作助手，帮我把工作周报写得清晰、简洁，突出完成事项、风险和下周计划。" onChange={(event) => { setSource(event.target.value); clearComparison(); }} />
          <footer><span>{sourceEntry ? `来自：${sourceEntry.title}` : "写下目标，或从「我的提示词」载入"}</span><button disabled={!source || !!busy} title="复制原始提示词" aria-label="复制原始提示词" onClick={() => void copy(source)}><Copy size={15} /></button></footer>
        </section>
        <section className="prompt-pane prompt-result-pane" aria-busy={busy === "optimize" || busy === "iterate"}>
          <header><label htmlFor="prompt-output">优化结果</label>{history.length > 0 && <GlassSelect aria-label="本次优化版本" disabled={!!busy} value={versionId} options={history.map((item, index) => ({ value: item.id, label: `${index + 1}. ${item.label}` }))} onValueChange={(value) => { const chosen = history.find((item) => item.id === value); if (chosen) { setOutput(chosen.content); setResult(chosen.result); setVersionId(value); clearComparison(); } }} />}</header>
          <textarea id="prompt-output" value={busy === "optimize" || busy === "iterate" ? partial : output} maxLength={PROMPT_LIMIT} readOnly={!!busy} placeholder="优化后的提示词会显示在这里，你也可以直接修改，再继续迭代。" onChange={(event) => { setOutput(event.target.value); setVersionId(""); setResult(undefined); clearComparison(); }} />
          <footer><RunMeta result={result} /><div><button disabled={!output || !!busy} aria-label="复制优化结果" title="复制优化结果" onClick={() => void copy(output)}><Copy size={15} /></button><button disabled={!native || !output.trim() || !!busy} onClick={() => setSaving(true)}><Save size={15} />{sourceEntry ? "更新提示词" : "保存到提示词库"}</button></div></footer>
        </section>
      </div>
      <div className="prompt-instructions">
        <label htmlFor="prompt-instruction">优化要求<span>首次优化可选；继续迭代时填写具体改进意见</span></label>
        <textarea id="prompt-instruction" rows={2} maxLength={4000} disabled={!!busy} placeholder="例如：保留 {{主题}} 变量，增加输出示例，语气更自然。" value={instruction} onChange={(event) => setInstruction(event.target.value)} />
        <div className="prompt-action-bar">
          <span className="prompt-hint">保留原意与变量；优化时不执行原始任务。</span>
          <div>{busy ? <button disabled={stopping} onClick={() => void cancel()}><Square size={15} />{stopping ? "正在停止…" : "停止生成"}</button> : <>
            <button disabled={!native || !chosenModel || !output.trim() || !instruction.trim()} onClick={() => void optimize("iterate")}><RotateCcw size={15} />继续迭代</button>
            <button className="primary" disabled={!native || !chosenModel || !source.trim()} onClick={() => void optimize("optimize")}><WandSparkles size={15} />开始优化</button>
          </>}</div>
        </div>
        {busy && <p role="status" className="prompt-status"><LoaderCircle size={14} className="prompt-spinning" />{busy === "test" ? "正在分别测试原文与优化结果…" : busy === "iterate" ? "正在按改进意见迭代…" : "正在优化提示词…"}</p>}
        {error && <p role="alert" className="prompt-error">{error}</p>}
      </div>
      <details className="prompt-test">
        <summary><FlaskConical size={17} />效果对比<span>用同一模型与输入，检验优化前后的回答</span></summary>
        <div className="prompt-test-controls">
          <label>测试模型<GlassSelect aria-label="测试模型" disabled={!!busy || !options.length} value={chosenTestModel} options={options} placeholder="暂无可用模型" onValueChange={(value) => { setTestModelId(value); clearComparison(); }} /></label>
          <label htmlFor="prompt-test-input">{kind === "system" ? "测试输入" : "补充输入（可选）"}<textarea id="prompt-test-input" rows={2} maxLength={PROMPT_LIMIT} disabled={!!busy} value={testInput} placeholder={kind === "system" ? "输入一段真实场景的问题，分别交给两个提示词回答" : "需要补充的素材或背景；留空则直接执行提示词"} onChange={(event) => { setTestInput(event.target.value); clearComparison(); }} /></label>
          <button disabled={!native || !!busy || !source.trim() || !output.trim() || !chosenTestModel || (kind === "system" && !testInput.trim())} onClick={() => void compare()}><FlaskConical size={15} />开始对比</button>
        </div>
        {hasComparison && <div className="prompt-comparison">{comparison.map((item, index) => <section key={index}><header><h3>{index === 0 ? "原始提示词的回答" : "优化提示词的回答"}</h3>{item.result && <Check size={14} />}</header><pre>{item.content || (busy === "test" ? "等待模型回答…" : "未完成回答")}</pre>{item.error && <p role="alert" className="prompt-error">{item.error}，此回答未完成。</p>}<RunMeta result={item.result} /></section>)}</div>}
      </details>
    </div>
    {tab === "library" && <PromptLibrary entries={entries} loading={loading} error={libraryError} onRetry={() => void reload()} onSaved={upsert} onDeleted={removeEntry} onUse={usePrompt} notify={notify} />}
    {saving && <PromptEditor entry={sourceEntry ? { ...sourceEntry, content: output, kind } : undefined} initial={{ content: output, kind }} onClose={() => setSaving(false)} onSaved={(entry) => { upsert(entry); setSourceId(entry.id); setSaving(false); notify("已保存到我的提示词"); }} />}
  </div>;
}

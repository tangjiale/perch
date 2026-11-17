import { useEffect, useRef, useState, type FormEvent } from "react";
import GlassSelect from "../components/GlassSelect";
import Modal from "../components/Modal";
import { api } from "../lib/api";
import type { BugDraft, Project, ZentaoBug, ZentaoBugCreateOptions } from "../lib/types";

const categories = [
  ["codeerror", "代码错误"],
  ["config", "配置相关"],
  ["install", "安装部署"],
  ["security", "安全问题"],
  ["performance", "性能问题"],
  ["standard", "标准规范"],
  ["automation", "测试脚本"],
  ["designdefect", "设计缺陷"],
  ["others", "其他"],
].map(([value, label]) => ({ value, label }));
const levels = [1, 2, 3, 4].map(n => ({ value: String(n), label: String(n) }));
const noProject = { value: "", label: "不选禅道项目（本地 BUG）" };
const errorText = (error: unknown) => error instanceof Error ? error.message : String(error);

interface Props {
  workspaceId: string;
  projects: Project[];
  bug?: ZentaoBug;
  onClose: () => void;
  onSaved: (message: string, status?: ZentaoBug["status"]) => void;
}

export default function BugComposer({ workspaceId, projects, bug, onClose, onSaved }: Props) {
  const [draft, setDraft] = useState<BugDraft>(() => ({
    id: bug ? bug.id.replace(/^local-bug-/, "") : crypto.randomUUID(),
    title: bug?.title ?? "",
    steps: bug?.steps ?? "",
    bugType: bug?.bugType ?? "codeerror",
    severity: bug?.severity ?? 3,
    priority: bug?.priority ?? 3,
    deadline: bug?.deadline ?? "",
  }));
  const update = <K extends keyof BugDraft>(key: K, value: BugDraft[K]) =>
    setDraft(previous => ({ ...previous, [key]: value }));
  const [status, setStatus] = useState<ZentaoBug["status"]>(bug?.status ?? "active");
  const [project, setProject] = useState("");
  const [product, setProduct] = useState("");
  const [execution, setExecution] = useState("");
  const [assignee, setAssignee] = useState("");
  const [build, setBuild] = useState("trunk");
  const [mailto, setMailto] = useState<string[]>([]);
  const [options, setOptions] = useState<ZentaoBugCreateOptions | null>(null);
  const [optionError, setOptionError] = useState("");
  const [attempt, setAttempt] = useState(0);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const submitting = useRef(false);
  const selectedProject = projects.find(item => item.id === project);

  useEffect(() => {
    if (!project || !selectedProject) return;
    let cancelled = false;
    void api.zentaoBugCreateOptions(workspaceId, project).then(result => {
      if (cancelled) return;
      setOptions(result);
      if (result.products.length === 1) setProduct(result.products[0].id);
    }).catch(reason => {
      if (!cancelled) setOptionError(errorText(reason));
    });
    return () => { cancelled = true; };
  }, [workspaceId, project, selectedProject?.id, attempt]);

  function selectProject(value: string) {
    if (value === project) return;
    setProject(value);
    setOptions(null);
    setOptionError("");
    setError("");
    setProduct("");
    setExecution("");
    setAssignee("");
    setMailto([]);
    setBuild("trunk");
  }

  const executions = options?.executions.filter(item => !item.productIds || item.productIds.includes(product)) ?? [];
  const users = options?.assignees.map(({ account, realname }) => ({
    value: account, label: realname && realname !== account ? `${realname}（${account}）` : account,
  })) ?? [];
  const builds = build.split(/[,，]/).map(value => value.trim()).filter(Boolean);
  const remoteReady = !!(selectedProject && options && product && assignee && builds.length
    && (!execution || executions.some(item => item.id === execution)));
  const canSave = !!draft.title.trim() && (!project || remoteReady);

  async function submit(event: FormEvent) {
    event.preventDefault();
    if (submitting.current || !canSave) return;
    submitting.current = true;
    setBusy(true);
    setError("");
    const base = { ...draft, title: draft.title.trim(), deadline: draft.deadline || undefined };
    let message: string;
    let savedStatus: ZentaoBug["status"] | undefined;
    try {
      if (project) {
        const result = await api.zentaoBugCreate(workspaceId, {
          ...base, projectId: project, productId: product, executionId: execution || undefined,
          assignedTo: assignee, openedBuild: builds, mailto,
        });
        message = result.assignedToSelf
          ? `禅道 BUG #${result.remoteId} 已创建`
          : `禅道 BUG #${result.remoteId} 已创建并指派给 ${assignee}，请在禅道查看`;
        if (result.assignedToSelf) savedStatus = "active";
      } else {
        const result = await api.localBugSave(workspaceId, {
          ...base, revision: bug?.revision, status,
        });
        message = bug ? "本地 BUG 已更新" : "本地 BUG 已创建";
        savedStatus = result.status;
      }
    } catch (reason) {
      setError(errorText(reason));
      setBusy(false);
      submitting.current = false;
      return;
    }
    onSaved(message, savedStatus);
  }

  return (
    <Modal title={bug ? "本地 BUG" : "新建 BUG"} wide busy={busy} onClose={onClose}>
      <form className="form-grid bugs-form" onSubmit={submit}>
        {error && <div className="bugs-full error-panel" role="alert">{error}</div>}
        <label className="bugs-full">
          BUG 标题
          <input autoFocus required maxLength={255} value={draft.title} disabled={busy}
            placeholder="一句话说明遇到的问题" onChange={event => update("title", event.target.value)} />
        </label>
        {!bug && (
          <label className="bugs-full">
            禅道项目
            <GlassSelect aria-label="禅道项目" searchable value={project} onValueChange={selectProject}
              disabled={busy} options={[
                noProject,
                ...(project && !selectedProject ? [{ value: project, label: "项目已不可用", disabled: true }] : []),
                ...projects.map(item => ({ value: item.id, label: `${item.name} #${item.remoteId}` })),
              ]} />
            <small>{project
              ? "提交后会在禅道创建 BUG；指派给他人的 BUG 可在禅道查看。"
              : projects.length ? "留空仅保存到本机。" : "当前没有可选禅道项目。同步项目后可在这里选择。"}</small>
          </label>
        )}
        {project && !selectedProject && <p className="bugs-full error" role="alert">所选项目已不可用，请重新选择。</p>}
        {selectedProject && !options && (
          <div className="bugs-full" role={optionError ? "alert" : "status"}>
            {optionError ? <>
              <p className="error">{optionError}</p>
              <button type="button" onClick={() => { setOptionError(""); setAttempt(n => n + 1); }}>
                重新加载项目选项
              </button>
            </> : <p className="muted">正在读取产品、执行和指派人…</p>}
          </div>
        )}
        {project && options && (
          <>
            <label>
              所属产品（必选）
              <GlassSelect aria-label="所属产品" value={product} searchable disabled={busy}
                placeholder="请选择产品" onValueChange={value => { setProduct(value); setExecution(""); }}
                options={options.products.map(item => ({ value: item.id, label: item.name }))} />
              {!options.products.length && <small>此项目没有可用的关联产品。</small>}
            </label>
            <label>
              所属执行
              <GlassSelect aria-label="所属执行" value={execution} searchable disabled={busy || !product}
                onValueChange={setExecution}
                options={[{ value: "", label: "不关联执行" }, ...executions.map(item => ({ value: item.id, label: item.name }))]} />
            </label>
            <label>
              指派给（必选）
              <GlassSelect aria-label="指派给" value={assignee} searchable disabled={busy}
                placeholder="按姓名或账号选择" options={users} onValueChange={setAssignee} />
            </label>
            <label>
              影响版本
              <input required value={build} disabled={busy} onChange={event => setBuild(event.target.value)} />
              <small>主干用 trunk；指定版本时填写禅道版本编号，用逗号分隔。</small>
            </label>
            <div className="bugs-full bug-mailto">
              <label>
                抄送给（可选）
                <GlassSelect aria-label="添加抄送人" value="" searchable disabled={busy}
                  placeholder="按姓名或账号添加"
                  options={users.filter(user => !mailto.includes(user.value))}
                  onValueChange={value => { if (value) setMailto(previous => [...previous, value]); }} />
              </label>
              {mailto.length > 0 && <div className="bug-mailto-list">
                {mailto.map(account => <button type="button" key={account} disabled={busy}
                  aria-label={`移除抄送 ${account}`} onClick={() => setMailto(previous => previous.filter(item => item !== account))}>
                  {users.find(item => item.value === account)?.label ?? account} ×
                </button>)}
              </div>}
            </div>
          </>
        )}
        <label>
          BUG 类型
          <GlassSelect aria-label="BUG 类型" value={draft.bugType} disabled={busy}
            options={categories} onValueChange={value => update("bugType", value)} />
        </label>
        <label>
          严重程度
          <GlassSelect aria-label="严重程度" value={String(draft.severity)} disabled={busy}
            options={levels} onValueChange={value => update("severity", Number(value))} />
        </label>
        <label>
          优先级
          <GlassSelect aria-label="优先级" value={String(draft.priority)} disabled={busy}
            options={levels.map(item => ({ ...item, label: `P${item.value}` }))}
            onValueChange={value => update("priority", Number(value))} />
        </label>
        <label>
          截止日期（可选）
          <input type="date" value={draft.deadline} disabled={busy} onChange={event => update("deadline", event.target.value)} />
        </label>
        {bug && <label>
          状态
          <GlassSelect aria-label="本地 BUG 状态" value={status} disabled={busy}
            options={[{ value: "active", label: "待解决" }, { value: "resolved", label: "已解决" }, { value: "closed", label: "已关闭" }]}
            onValueChange={value => setStatus(value as ZentaoBug["status"])} />
        </label>}
        <label className="bugs-full">
          复现步骤
          <textarea rows={6} maxLength={30000} value={draft.steps} disabled={busy}
            placeholder={"[步骤]\n\n[实际结果]\n\n[预期结果]"}
            onChange={event => update("steps", event.target.value)} />
        </label>
        <footer className="bugs-detail-footer">
          <button type="button" onClick={onClose} disabled={busy}>取消</button>
          <button type="submit" className="primary" disabled={busy || !canSave}>
            {busy ? "正在保存…" : project ? "提交到禅道" : "保存本地 BUG"}
          </button>
        </footer>
      </form>
    </Modal>
  );
}

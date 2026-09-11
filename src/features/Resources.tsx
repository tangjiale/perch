import { useState, useRef } from "react";
import GlassSelect from "../components/GlassSelect";
import SkillCatalog from "../components/SkillCatalog";
import { canEditSkill } from "../lib/skill-sources";
import AppLogoPicker from "../components/AppLogoPicker";
import {
  Plus,
  Pencil,
  Trash2,
  Star,
  ExternalLink,
  Puzzle,
  Bot,
  Library,
  FileText,
  Upload,
  Play,
  Settings2,
  Search,
  Copy,
  Download,
  RefreshCw,
} from "lucide-react";
import { useNavigate } from "react-router-dom";
import { open, save } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { load as loadYaml, JSON_SCHEMA } from "js-yaml";
import { api, command } from "../lib/api";
import Modal from "../components/Modal";
import type {
  PageProps,
  EntityKind,
  Application,
  Skill,
  Knowledge,
  Agent,
} from "../lib/types";

export function Apps({
  data,
  refresh,
  notify,
  search = "",
}: PageProps & { search?: string }) {
  const [edit, setEdit] = useState<Application | null>(null),
    [category, setCategory] = useState("all"),
    [manager, setManager] = useState(false),
    [categoryName, setCategoryName] = useState(""),
    [error, setError] = useState("");
  async function save(e: React.FormEvent) {
    e.preventDefault();
    try {
      await api.save("app", edit!);
      await refresh();
      setEdit(null);
    } catch (e) {
      setError((e as Error).message);
    }
  }
  async function remove(kind: EntityKind, id: string, revision?: number) {
    try {
      await api.remove(kind, id, revision);
      await refresh();
      if (kind === "category" && category === id) setCategory("all");
    } catch (e) {
      notify((e as Error).message);
    }
  }
  return (
    <>
      <div className="page-heading">
        <div>
          <h1>应用</h1>
          <p>常用系统与团队工具</p>
        </div>
        <button
          className="primary"
          onClick={() => {
            setError("");
            setEdit({
              id: crypto.randomUUID(),
              name: "",
              url: "https://",
              description: "",
              favorite: false,
              sortOrder: Date.now(),
            });
          }}
        >
          <Plus size={16} />
          添加应用
        </button>
      </div>
      <div className="app-category-toolbar">
        <div
          className="app-category-filters"
          role="group"
          aria-label="应用分类"
        >
          {[
            { value: "all", label: "全部" },
            ...data.categories.map((c) => ({ value: c.id, label: c.name })),
          ].map((item) => (
            <button
              key={item.value}
              className="app-category-filter"
              aria-pressed={category === item.value}
              onClick={() => setCategory(item.value)}
            >
              {item.label}
            </button>
          ))}
        </div>
        <div className="app-category-actions">
          <button
            className="app-category-filter"
            aria-pressed={category === "favorite"}
            onClick={() => setCategory("favorite")}
          >
            <Star size={14} aria-hidden="true" />
            我的收藏
          </button>
          <button
            className="app-category-manage"
            onClick={() => setManager(true)}
          >
            <Settings2 size={15} aria-hidden="true" />
            管理分类
          </button>
        </div>
      </div>
      <div className="app-grid">
        {data.apps
          .filter(
            (a) =>
              (category === "all" ||
                (category === "favorite" && a.favorite) ||
                a.categoryId === category) &&
              `${a.name} ${a.description} ${a.url}`.includes(search),
          )
          .map((a) => (
            <article className="app-card" key={a.id}>
              <div className="resource-card-top">
                <div className="app-logo">
                  {a.logo ? <img src={a.logo} alt="" /> : a.name.slice(0, 1)}
                </div>
                <button
                  title={a.favorite ? "取消收藏" : "收藏"}
                  className={"icon-button " + (a.favorite ? "favorite" : "")}
                  onClick={async () => {
                    try {
                      await api.save("app", { ...a, favorite: !a.favorite });
                      await refresh();
                    } catch (e) {
                      notify((e as Error).message);
                    }
                  }}
                >
                  <Star size={16} fill={a.favorite ? "currentColor" : "none"} />
                </button>
              </div>
              <div className="app-card-title">
                <h3>{a.name}</h3>
                <span className="tag app-category-label">
                  {data.categories.find((c) => c.id === a.categoryId)?.name || "未分类"}
                </span>
              </div>
              <p title={a.description}>{a.description || "暂无描述"}</p>
              <small>{a.url}</small>
              <div className="card-actions">
                <button
                  onClick={() => openUrl(a.url).catch((e) => notify(String(e)))}
                >
                  <ExternalLink size={14} />
                  打开
                </button>
                <button
                  className="icon-button"
                  title="编辑应用"
                  onClick={() => {
                    setError("");
                    setEdit(a);
                  }}
                >
                  <Pencil size={15} />
                </button>
              </div>
            </article>
          ))}
      </div>
      {!data.apps.length && (
        <div className="empty">
          <ExternalLink />
          <p>还没有应用入口</p>
        </div>
      )}
      {edit && (
        <Modal
          title={edit.revision ? "编辑应用" : "添加应用"}
          onClose={() => setEdit(null)}
        >
          <form className="form-grid" onSubmit={save}>
            <label>
              名称
              <input
                required
                maxLength={60}
                value={edit.name}
                onChange={(e) => setEdit({ ...edit, name: e.target.value })}
              />
            </label>
            <label>
              网址
              <input
                required
                type="url"
                value={edit.url}
                onChange={(e) => setEdit({ ...edit, url: e.target.value })}
              />
            </label>
            <label>
              描述
              <textarea
                rows={3}
                maxLength={240}
                value={edit.description}
                onChange={(e) =>
                  setEdit({ ...edit, description: e.target.value })
                }
              />
            </label>
            <label>
              分类
              <div className="inline-field">
                <GlassSelect
                  aria-label="应用所属分类"
                  value={edit.categoryId || ""}
                  onValueChange={(categoryId) =>
                    setEdit({
                      ...edit,
                      categoryId: categoryId || undefined,
                    })
                  }
                  options={[
                    { value: "", label: "未分类" },
                    ...data.categories.map((c) => ({
                      value: c.id,
                      label: c.name,
                    })),
                  ]}
                />
                <button
                  type="button"
                  className="text-link"
                  onClick={() => setManager(true)}
                >
                  管理分类
                </button>
              </div>
            </label>
            <AppLogoPicker
              key={edit.id}
              url={edit.url}
              value={edit.logo}
              onChange={(logo) =>
                setEdit((current) =>
                  current?.id === edit.id ? { ...current, logo } : current,
                )
              }
            />
            {error && (
              <p role="alert" className="error">
                {error}
              </p>
            )}
            <footer>
              {edit.revision && (
                <button
                  type="button"
                  className="danger"
                  onClick={async () => {
                    if (confirm("移除这个应用入口？")) {
                      await remove("app", edit.id, edit.revision);
                      setEdit(null);
                    }
                  }}
                >
                  <Trash2 size={15} />
                  移除
                </button>
              )}
              <button className="primary">保存</button>
            </footer>
          </form>
        </Modal>
      )}
      {manager && (
        <Modal title="管理分类" onClose={() => setManager(false)}>
          <div className="category-list">
            {data.categories.map((c) => (
              <div key={c.id}>
                <span>{c.name}</span>
                <button
                  title="修改分类"
                  className="icon-button"
                  onClick={async () => {
                    const name = prompt("分类名称", c.name);
                    if (name)
                      try {
                        await api.save("category", { ...c, name });
                        await refresh();
                      } catch (e) {
                        notify((e as Error).message);
                      }
                  }}
                >
                  <Pencil size={15} />
                </button>
                <button
                  title="删除分类"
                  className="icon-button"
                  onClick={async () => {
                    if (!confirm("删除此分类？有关联应用时请先修改应用分类。"))
                      return;
                    await remove("category", c.id, c.revision);
                  }}
                >
                  <Trash2 size={15} />
                </button>
              </div>
            ))}
          </div>
          <form
            className="inline-field"
            onSubmit={async (e) => {
              e.preventDefault();
              try {
                await api.save("category", {
                  id: crypto.randomUUID(),
                  name: categoryName,
                });
                await refresh();
                setCategoryName("");
              } catch (e) {
                notify((e as Error).message);
              }
            }}
          >
            <input
              required
              aria-label="新分类名称"
              placeholder="分类名称"
              value={categoryName}
              onChange={(e) => setCategoryName(e.target.value)}
            />
            <button className="primary">
              <Plus size={15} />
              添加
            </button>
          </form>
        </Modal>
      )}
    </>
  );
}

export function Skills({
  data,
  refresh,
  notify,
  search = "",
}: PageProps & { search?: string }) {
  const [edit, setEdit] = useState<Skill | null>(null),
    [error, setError] = useState("");
  const [loadingLocal, setLoadingLocal] = useState(false);
  const [toggling, setToggling] = useState<string | null>(null);
  async function readLocalSkills() {
    if (loadingLocal) return;
    setLoadingLocal(true);
    try {
      const result = await command<{ added: number; existing: number; skipped: number; warnings: string[] }>("local_skills_import");
      await refresh();
      notify(`已读取本机技能：新增 ${result.added} 项，已有 ${result.existing} 项${result.warnings.length ? `；${result.warnings.length} 项读取提示` : ""}`);
      setError(result.warnings.join("；"));
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally { setLoadingLocal(false); }
  }
  const input = useRef<HTMLInputElement>(null);
  async function importFile(file: File) {
    try {
      if (file.size > 256 * 1024) throw Error("技能文件不能超过 256 KB");
      const raw = await file.text();
      let value: Record<string, unknown>;
      if (file.name.endsWith(".json")) {
        value = JSON.parse(raw);
      } else {
        const match = raw.match(/^---\r?\n([\s\S]*?)\r?\n---\r?\n([\s\S]*)$/);
        const meta = match ? loadYaml(match[1], { schema: JSON_SCHEMA }) : {};
        if (meta && typeof meta !== "object")
          throw Error("技能元信息必须是对象");
        value = { ...(meta as object), content: match ? match[2] : raw };
      }
      if (typeof value.content !== "string") throw Error("缺少技能正文");
      setEdit({
        id: crypto.randomUUID(),
        name:
          typeof value.name === "string"
            ? value.name
            : file.name.replace(/\.[^.]+$/, ""),
        description:
          typeof value.description === "string" ? value.description : "",
        content: value.content,
        enabled: true,
      });
      setError("");
    } catch (e) {
      notify((e as Error).message);
    }
  }
  return (
    <>
      <div className="page-heading">
        <div>
          <h2>技能</h2>
          <p>可复用的指令与工作方法</p>
        </div>
        <div className="actions">
          <button disabled={loadingLocal || toggling !== null} onClick={() => void readLocalSkills()}>
            <RefreshCw size={15} />{loadingLocal ? "读取中…" : "读取本机技能"}
          </button>
          <button onClick={() => input.current?.click()}>
            <Upload size={15} />
            导入
          </button>
          <button
            className="primary"
            onClick={() => {
              setError("");
              setEdit({
                id: crypto.randomUUID(),
                name: "",
                description: "",
                content: "",
                enabled: true,
              });
            }}
          >
            <Plus size={15} />
            新建技能
          </button>
          <input
            ref={input}
            hidden
            type="file"
            accept=".md,.json"
            onChange={(e) => {
              const f = e.target.files?.[0];
              if (f) void importFile(f);
              e.target.value = "";
            }}
          />
        </div>
      </div>
      <p className="muted">读取本机 Codex、Claude 和通用 Agent 技能目录。新读取的技能默认停用，启用后可在 Agent 中关联使用；不会修改原文件或执行附带脚本。</p>
      {error && !edit && <p className="error" role="alert">{error}</p>}
      <SkillCatalog skills={data.skills} search={search} busy={loadingLocal || toggling !== null}
        onToggleGroup={async (skills, enabled) => {
          setToggling("group");
          try {
            await command("local_skills_set_enabled", {
              skills: skills.map((skill) => ({ id: skill.id, revision: skill.revision })), enabled,
            });
            await refresh();
            setError("");
          } catch (cause) { setError(cause instanceof Error ? cause.message : String(cause)); }
          finally { setToggling(null); }
        }}
        renderSkill={(s) => (
            <article className="resource-card" key={s.id}>
              <div className="resource-card-top">
                <Puzzle />
                <button type="button" className="model-status-switch" role="switch"
                  aria-checked={s.enabled} aria-label={`${s.enabled ? "停用" : "启用"}技能 ${s.name}`}
                  disabled={toggling !== null || loadingLocal}
                  onClick={async () => {
                    setToggling(s.id);
                    try {
                      await api.save("skill", { ...s, enabled: !s.enabled });
                      await refresh();
                      setError("");
                    } catch (cause) { setError(cause instanceof Error ? cause.message : String(cause)); }
                    finally { setToggling(null); }
                  }}>
                  <span className="model-switch-track" aria-hidden="true"><span /></span>
                  {s.enabled ? "已启用" : "已停用"}
                </button>
              </div>
              <h3>{s.name}</h3>
              <p>{s.description || "暂无描述"}</p>
              {s.sourcePath && <small className="muted" title={s.sourcePath} style={{ display: "block", overflowWrap: "anywhere" }}>本机 · {s.sourcePath}</small>}
              <div className="card-actions">
                <button
                  onClick={() => {
                    setError("");
                    setEdit(s);
                  }}
                >
                  {canEditSkill(s) ? <Pencil size={14} /> : <FileText size={14} />}
                  {canEditSkill(s) ? "编辑" : "查看内容"}
                </button>
                <small>修订 {s.revision}</small>
              </div>
            </article>
        )}
      />
      {!data.skills.length && (
        <div className="empty">
          <Puzzle />
          <p>还没有技能</p>
        </div>
      )}
      {edit && (
        <Modal
          title={!canEditSkill(edit) ? "查看技能内容" : edit.revision ? "编辑技能" : "新建 / 导入技能"}
          wide
          onClose={() => setEdit(null)}
        >
          <form
            className="form-grid"
            onSubmit={async (e) => {
              e.preventDefault();
              if (!canEditSkill(edit)) return;
              try {
                await api.save("skill", edit);
                await refresh();
                setEdit(null);
              } catch (e) {
                setError((e as Error).message);
              }
            }}
          >
            <label>
              名称
              <input
                required
                readOnly={!canEditSkill(edit)}
                value={edit.name}
                onChange={(e) => setEdit({ ...edit, name: e.target.value })}
              />
            </label>
            <label>
              描述
              <input
                readOnly={!canEditSkill(edit)}
                value={edit.description}
                onChange={(e) =>
                  setEdit({ ...edit, description: e.target.value })
                }
              />
            </label>
            <label>
              技能指令
              <textarea
                required
                rows={12}
                readOnly={!canEditSkill(edit)}
                value={edit.content}
                onChange={(e) => setEdit({ ...edit, content: e.target.value })}
              />
            </label>
            {canEditSkill(edit) && <label className="check-label">
              <input
                type="checkbox"
                checked={edit.enabled}
                onChange={(e) =>
                  setEdit({ ...edit, enabled: e.target.checked })
                }
              />
              启用技能
            </label>}
            {error && (
              <p role="alert" className="error">
                {error}
              </p>
            )}
            <footer>
              {edit.revision && canEditSkill(edit) && (
                <button
                  type="button"
                  className="danger"
                  onClick={async () => {
                    if (!confirm("删除这个技能？")) return;
                    try {
                      await api.remove("skill", edit.id, edit.revision);
                      await refresh();
                      setEdit(null);
                    } catch (e) {
                      setError((e as Error).message);
                    }
                  }}
                >
                  <Trash2 size={14} />
                  删除
                </button>
              )}
              {canEditSkill(edit) ? <button className="primary">确认保存</button> : <button type="button" onClick={() => setEdit(null)}>关闭</button>}
            </footer>
          </form>
        </Modal>
      )}
    </>
  );
}

export function Agents({
  data,
  refresh,
  notify,
  search = "",
}: PageProps & { search?: string }) {
  const navigate = useNavigate();
  const [edit, setEdit] = useState<Agent | null>(null),
    [error, setError] = useState("");
  const models = data.models.filter(
    (m) =>
      m.capability !== "embedding" &&
      m.enabled &&
      data.providers.find((p) => p.id === m.providerId)?.enabled,
  );
  async function start(agent: Agent) {
    try {
      const model = data.models.find((m) => m.id === agent.modelId);
      const provider = data.providers.find((p) => p.id === model?.providerId);
      const c = await api.save("conversation", {
        id: crypto.randomUUID(),
        title: "新会话",
        agentId: agent.id,
        agentSnapshot: {
          ...agent,
          model,
          provider,
          skills: data.skills.filter((s) => s.enabled && agent.skillIds.includes(s.id)),
        },
      });
      await refresh();
      navigate("/chat?conversation=" + c.id);
    } catch (e) {
      notify((e as Error).message);
    }
  }
  return (
    <>
      <div className="page-heading">
        <div>
          <h2>Agent</h2>
          <p>为不同工作配置独立助手</p>
        </div>
        <button
          className="primary"
          onClick={() => {
            setError("");
            setEdit({
              id: crypto.randomUUID(),
              name: "",
              description: "",
              systemPrompt: "",
              modelId: models[0]?.id || "",
              temperature: 0.5,
              maxTokens: 4096,
              enabled: true,
              skillIds: [],
            });
          }}
        >
          <Plus size={15} />
          新建 Agent
        </button>
      </div>
      <div className="resource-grid">
        {data.agents
          .filter((a) => (a.name + a.description).includes(search))
          .map((a) => (
            <article className="resource-card" key={a.id}>
              <div className="resource-card-top">
                <Bot />
                <span className={"tag " + (a.enabled ? "done" : "closed")}>
                  {a.enabled ? "已启用" : "已停用"}
                </span>
              </div>
              <h3>{a.name}</h3>
              <p>{a.description || "暂无描述"}</p>
              <small>
                {data.models.find((m) => m.id === a.modelId)?.name ||
                  "模型不可用"}{" "}
                · {a.skillIds.length} 项技能
              </small>
              <div className="card-actions">
                <button
                  onClick={() => {
                    setError("");
                    setEdit(a);
                  }}
                >
                  <Settings2 size={14} />
                  配置
                </button>
                <button disabled={!a.enabled} onClick={() => void start(a)}>
                  <Play size={14} />
                  聊天
                </button>
              </div>
            </article>
          ))}
      </div>
      {!data.agents.length && (
        <div className="empty">
          <Bot />
          <p>还没有 Agent</p>
        </div>
      )}
      {edit && (
        <Modal title="Agent 配置" wide onClose={() => setEdit(null)}>
          <form
            className="form-grid"
            onSubmit={async (e) => {
              e.preventDefault();
              try {
                await api.save("agent", edit);
                await refresh();
                setEdit(null);
              } catch (e) {
                setError((e as Error).message);
              }
            }}
          >
            <div className="two-fields">
              <label>
                名称
                <input
                  required
                  value={edit.name}
                  onChange={(e) => setEdit({ ...edit, name: e.target.value })}
                />
              </label>
              <label>
                对话模型
                <GlassSelect
                  aria-label="对话模型"
                  required
                  value={edit.modelId}
                  onValueChange={(modelId) => setEdit({ ...edit, modelId })}
                  options={[
                    { value: "", label: "选择模型" },
                    ...models.map((m) => ({ value: m.id, label: m.name })),
                  ]}
                />
              </label>
            </div>
            <label>
              描述
              <input
                value={edit.description}
                onChange={(e) =>
                  setEdit({ ...edit, description: e.target.value })
                }
              />
            </label>
            <label>
              系统提示词
              <textarea
                required
                rows={6}
                value={edit.systemPrompt}
                onChange={(e) =>
                  setEdit({ ...edit, systemPrompt: e.target.value })
                }
              />
            </label>
            <div className="two-fields">
              <label>
                Temperature
                <input
                  type="number"
                  step="0.1"
                  min="0"
                  max="2"
                  value={edit.temperature}
                  onChange={(e) =>
                    setEdit({ ...edit, temperature: Number(e.target.value) })
                  }
                />
              </label>
              <label>
                最大输出 Token
                <input
                  type="number"
                  min="1"
                  max="131072"
                  value={edit.maxTokens}
                  onChange={(e) =>
                    setEdit({ ...edit, maxTokens: Number(e.target.value) })
                  }
                />
              </label>
            </div>
            <fieldset>
              <legend>关联技能</legend>
              {data.skills
                .filter((s) => s.enabled)
                .map((s) => (
                  <label className="check-label" key={s.id}>
                    <input
                      type="checkbox"
                      checked={edit.skillIds.includes(s.id)}
                      onChange={(e) =>
                        setEdit({
                          ...edit,
                          skillIds: e.target.checked
                            ? [...edit.skillIds, s.id]
                            : edit.skillIds.filter((id) => id !== s.id),
                        })
                      }
                    />
                    {s.name}
                  </label>
                ))}
            </fieldset>
            <label className="check-label">
              <input
                type="checkbox"
                checked={edit.enabled}
                onChange={(e) =>
                  setEdit({ ...edit, enabled: e.target.checked })
                }
              />
              启用 Agent
            </label>
            {error && (
              <p className="error" role="alert">
                {error}
              </p>
            )}
            <footer>
              {edit.revision && (
                <button
                  type="button"
                  onClick={() =>
                    setEdit({
                      ...edit,
                      id: crypto.randomUUID(),
                      revision: undefined,
                      name: edit.name + " 副本",
                    })
                  }
                >
                  <Copy size={14} />
                  复制
                </button>
              )}
              {edit.revision && (
                <button
                  type="button"
                  className="danger"
                  onClick={async () => {
                    if (!confirm("删除这个 Agent？已有会话保留配置快照。"))
                      return;
                    try {
                      await api.remove("agent", edit.id, edit.revision);
                      await refresh();
                      setEdit(null);
                    } catch (e) {
                      setError((e as Error).message);
                    }
                  }}
                >
                  <Trash2 size={15} />
                  删除
                </button>
              )}
              <button className="primary">保存</button>
            </footer>
          </form>
        </Modal>
      )}
    </>
  );
}

export function KnowledgePage({
  data,
  refresh,
  notify,
  search = "",
}: PageProps & { search?: string }) {
  const [selected, setSelected] = useState<string | null>(null),
    [edit, setEdit] = useState<Knowledge | null>(null),
    [busy, setBusy] = useState(false),
    [error, setError] = useState("");
  const kb = data.knowledge.find((k) => k.id === selected);
  const models = data.models.filter(
    (m) => m.capability === "embedding" && m.enabled,
  );
  const docs = data.documents.filter((d) => d.knowledgeId === selected);
  async function upload() {
    const paths = await open({
      multiple: true,
      filters: [
        {
          name: "文档",
          extensions: [
            "txt",
            "md",
            "pdf",
            "doc",
            "docx",
            "ppt",
            "pptx",
            "xls",
            "xlsx",
          ],
        },
      ],
    });
    if (!paths) return;
    setBusy(true);
    try {
      const n = await command<number>("knowledge_import", {
        knowledgeId: selected,
        paths: Array.isArray(paths) ? paths : [paths],
      });
      await refresh();
      notify(`已导入 ${n} 份文档`);
    } catch (e) {
      notify((e as Error).message);
    } finally {
      setBusy(false);
    }
  }
  return (
    <>
      <div className="page-heading">
        <div>
          <h2>{kb ? kb.name : "知识库"}</h2>
          <p>{kb ? kb.description : "让工作资料成为可检索的知识"}</p>
        </div>
        <div className="actions">
          {kb ? (
            <>
              <button onClick={() => setSelected(null)}>全部知识库</button>
              <button
                onClick={() => {
                  setEdit(kb);
                  setError("");
                }}
              >
                <Settings2 size={15} />
                配置
              </button>
              <button
                className="primary"
                disabled={busy}
                onClick={() => void upload()}
              >
                <Upload size={15} />
                上传文件
              </button>
            </>
          ) : (
            <button
              className="primary"
              onClick={() => {
                setError("");
                setEdit({
                  id: crypto.randomUUID(),
                  name: "",
                  description: "",
                  modelId: models[0]?.id || "",
                });
              }}
            >
              <Plus size={15} />
              新建知识库
            </button>
          )}
        </div>
      </div>
      {kb ? (
        <>
          <div className="toolbar">
            <span className="tag">
              {data.models.find((m) => m.id === kb.modelId)?.name ||
                "向量模型未配置"}
            </span>
            <span>{docs.length} 份文档</span>
            <button
              disabled={busy || !docs.length}
              onClick={async () => {
                if (
                  !confirm(
                    "将文档正文发送到已配置的向量模型服务建立索引，是否继续？",
                  )
                )
                  return;
                setBusy(true);
                try {
                  notify(
                    await command<string>("knowledge_index", {
                      knowledgeId: kb.id,
                    }),
                  );
                  await refresh();
                } catch (e) {
                  notify((e as Error).message);
                } finally {
                  setBusy(false);
                }
              }}
            >
              <Play size={14} />
              {busy ? "处理中…" : "建立 / 重建索引"}
            </button>
          </div>
          <div className="table-wrap">
            <table>
              <thead>
                <tr>
                  <th>文档</th>
                  <th>大小</th>
                  <th>状态</th>
                  <th>分块</th>
                  <th></th>
                </tr>
              </thead>
              <tbody>
                {docs.map((d) => (
                  <tr key={d.id}>
                    <td>
                      <FileText size={16} /> {d.name}
                      {d.error && <small className="error">{d.error}</small>}
                    </td>
                    <td>{(d.size / 1024).toFixed(1)} KB</td>
                    <td>
                      <span
                        className={
                          "tag " + (d.status === "ready" ? "done" : "")
                        }
                      >
                        {{
                          parsed: "待索引",
                          ready: "已索引",
                          failed: "失败",
                          pending_parser: "待解析",
                          needs_ocr: "需要文字识别",
                        }[d.status] || d.status}
                      </span>
                    </td>
                    <td>{d.chunks || 0}</td>
                    <td>
                      <button
                        title="导出原件"
                        className="icon-button"
                        disabled={busy}
                        onClick={async () => {
                          try {
                            const destination = await save({
                              defaultPath: d.name,
                            });
                            if (destination) {
                              await command("knowledge_export", {
                                documentId: d.id,
                                destination,
                              });
                              notify("原件已导出");
                            }
                          } catch (e) {
                            notify((e as Error).message);
                          }
                        }}
                      >
                        <Download size={15} />
                      </button>
                      {d.status !== "ready" && (
                        <button
                          title="重新解析"
                          className="icon-button"
                          disabled={busy}
                          onClick={async () => {
                            setBusy(true);
                            try {
                              await command("knowledge_retry", {
                                documentId: d.id,
                              });
                              await refresh();
                              notify("解析完成");
                            } catch (e) {
                              notify((e as Error).message);
                            } finally {
                              setBusy(false);
                            }
                          }}
                        >
                          <RefreshCw size={15} />
                        </button>
                      )}
                      <button
                        title="删除文档"
                        className="icon-button"
                        disabled={busy}
                        onClick={async () => {
                          if (!confirm("删除这份文档？")) return;
                          try {
                            await api.remove("document", d.id, d.revision);
                            await refresh();
                          } catch (e) {
                            notify((e as Error).message);
                          }
                        }}
                      >
                        <Trash2 size={15} />
                      </button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          {!docs.length && (
            <div className="empty">
              <Upload />
              <p>还没有文档</p>
            </div>
          )}
        </>
      ) : (
        <div className="resource-grid">
          {data.knowledge
            .filter((k) => (k.name + k.description).includes(search))
            .map((k) => (
              <button
                className="resource-card"
                key={k.id}
                onClick={() => setSelected(k.id)}
              >
                <Library />
                <h3>{k.name}</h3>
                <p>{k.description || "暂无描述"}</p>
                <small>
                  {data.documents.filter((d) => d.knowledgeId === k.id).length}{" "}
                  份文档 ·{" "}
                  {data.models.find((m) => m.id === k.modelId)?.name ||
                    "模型不可用"}
                </small>
              </button>
            ))}
        </div>
      )}
      {!kb && !data.knowledge.length && (
        <div className="empty">
          <Library />
          <p>还没有知识库</p>
        </div>
      )}
      {edit && (
        <Modal title="知识库配置" onClose={() => setEdit(null)}>
          <form
            className="form-grid"
            onSubmit={async (e) => {
              e.preventDefault();
              try {
                await api.save("knowledge", edit);
                await refresh();
                setEdit(null);
              } catch (e) {
                setError((e as Error).message);
              }
            }}
          >
            <label>
              名称
              <input
                required
                value={edit.name}
                onChange={(e) => setEdit({ ...edit, name: e.target.value })}
              />
            </label>
            <label>
              描述
              <textarea
                rows={3}
                value={edit.description}
                onChange={(e) =>
                  setEdit({ ...edit, description: e.target.value })
                }
              />
            </label>
            <label>
              向量模型
              <GlassSelect
                aria-label="向量模型"
                required
                value={edit.modelId}
                onValueChange={(modelId) => setEdit({ ...edit, modelId })}
                options={[
                  { value: "", label: "选择向量模型" },
                  ...models.map((m) => ({
                    value: m.id,
                    label: `${data.providers.find((p) => p.id === m.providerId)?.name || "未配置供应商"} · ${m.name}`,
                  })),
                ]}
              />
            </label>
            <p className="notice">
              索引时，文档正文会发送到所选模型供应商。修改向量模型后需要重新索引。
            </p>
            {error && <p className="error">{error}</p>}
            <footer>
              {edit.revision && (
                <button
                  type="button"
                  className="danger"
                  disabled={busy}
                  onClick={async () => {
                    if (
                      !confirm(
                        "删除这个知识库？存在文档或会话引用时需要先解除关联。",
                      )
                    )
                      return;
                    try {
                      await api.remove("knowledge", edit.id, edit.revision);
                      await refresh();
                      setEdit(null);
                      setSelected(null);
                    } catch (e) {
                      setError((e as Error).message);
                    }
                  }}
                >
                  <Trash2 size={15} />
                  删除知识库
                </button>
              )}
              <button className="primary">保存</button>
            </footer>
          </form>
        </Modal>
      )}
    </>
  );
}

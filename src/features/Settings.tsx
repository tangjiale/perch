import { useEffect, useRef, useState } from "react";
import { useSearchParams } from "react-router-dom";
import { Agents, Skills, KnowledgePage } from "./Resources";
import McpSettings from "./McpSettings";
import GeneralSettings from "./GeneralSettings";
import MailSettings from "./MailSettings";
import DingCalendarSettings from "./DingCalendarSettings";
import { open, save } from "@tauri-apps/plugin-dialog";
import { revealItemInDir, openUrl } from "@tauri-apps/plugin-opener";
import {
  Plus,
  Settings2,
  Server,
  Database,
  FolderOpen,
  Download,
  Upload,
  RefreshCw,
  Trash2,
  Pencil,
} from "lucide-react";
import { api, command } from "../lib/api";
import type {
  PageProps,
  Provider,
  Model,
  Connection,
  StorageInfo,
} from "../lib/types";
import Modal from "../components/Modal";
import GlassSelect from "../components/GlassSelect";
import ZentaoConnectionDialog from "../components/ZentaoConnectionDialog";
import ProviderModelsDialog from "../components/ProviderModelsDialog";
import ModelCapability from "../components/ModelCapability";
import "./model-actions.css";

export default function Settings({
  data,
  refresh,
  notify,
  info,
  onInfo,
  search = "",
}: PageProps & {
  info: StorageInfo;
  onInfo: () => Promise<unknown>;
  search?: string;
}) {
  const [params, setParams] = useSearchParams();
  const [catalogProvider, setCatalogProvider] = useState<Provider | null>(null);
  const [providerBusy, setProviderBusy] = useState(false);
  const providerInFlight = useRef(false);
  const modelInFlight = useRef(false);
  const [modelBusy, setModelBusy] = useState(false);
  const [removeModel, setRemoveModel] = useState<Model | null>(null);
  const [removeError, setRemoveError] = useState("");
  async function toggleModel(value: Model) {
    if (modelInFlight.current) return;
    modelInFlight.current = true;
    setModelBusy(true);
    try {
      await api.save("model", { ...value, enabled: !value.enabled });
      await refresh();
    } catch (error) {
      notify((error as Error).message);
    } finally {
      modelInFlight.current = false;
      setModelBusy(false);
    }
  }
  async function confirmModelRemoval() {
    if (!removeModel || modelInFlight.current) return;
    modelInFlight.current = true;
    setModelBusy(true);
    setRemoveError("");
    try {
      await api.remove("model", removeModel.id, removeModel.revision);
      setRemoveModel(null);
      await refresh();
      notify("模型已删除");
    } catch (error) {
      setRemoveError((error as Error).message);
      notify((error as Error).message);
    } finally {
      modelInFlight.current = false;
      setModelBusy(false);
    }
  }
  const [provider, setProvider] = useState<Provider | null>(null),
    [model, setModel] = useState<Model | null>(null),
    [secret, setSecret] = useState(""),
    [error, setError] = useState(""),
    [busy, setBusy] = useState(false);
  const [zentaoSaving, setZentaoSaving] = useState(false);
  const [zentaoFormVersion, setZentaoFormVersion] = useState(0);
  const [newZentao] = useState<Connection>(() => ({
    id: crypto.randomUUID(),
    name: "公司禅道",
    baseUrl: "",
    apiVersion: "v1",
    enabled: true,
    managementEnabled: false,
  }));
  const zentao = data.connections[0] || newZentao;
  const tabs = [
    ["zentao", "禅道连接"],
    ["models", "大模型管理"],
    ["mail", "邮箱设置"],
    ["ding-calendar", "钉钉日历"],
    ["agents", "Agent"],
    ["knowledge", "知识库"],
    ["skills", "技能"],
    ["mcp", "MCP"],
    ["storage", "数据与备份"],
    ["general", "通用设置"],
    ["about", "关于"],
  ];
  const tab = tabs.some(([key]) => key === params.get("tab"))
    ? params.get("tab")!
    : "zentao";
  const setTab = (key: string) => setParams({ tab: key });
  useEffect(() => {
    document
      .getElementById(`settings-tab-${tab}`)
      ?.scrollIntoView({ block: "nearest", inline: "nearest" });
  }, [tab]);
  async function run(fn: () => Promise<unknown>) {
    setBusy(true);
    try {
      const result = await fn();
      await refresh();
      await onInfo();
      if (typeof result === "string") notify(result);
      else notify("操作完成");
    } catch (e) {
      notify((e as Error).message);
    } finally {
      setBusy(false);
    }
  }
  async function storeProvider(e: React.FormEvent) {
    e.preventDefault();
    if (providerInFlight.current) return;
    const fetchModels =
      (e.nativeEvent as SubmitEvent).submitter?.getAttribute("value") ===
      "models";
    providerInFlight.current = true;
    setProviderBusy(true);
    setError("");
    try {
      const p = await api.save("provider", provider!);
      // 保存后立即接收 revision；密钥保存或刷新失败时重试不会重复创建供应商。
      setProvider(p);
      if (secret) await command("credential_set", { id: p.id, secret });
      setSecret("");
      try {
        await refresh();
      } catch {
        notify("供应商已保存，页面刷新失败，请稍后刷新。");
      }
      if (fetchModels) setCatalogProvider(p);
      else setProvider(null);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      providerInFlight.current = false;
      setProviderBusy(false);
    }
  }
  return (
    <>
      <div className="page-heading">
        <div>
          <h1>设置</h1>
          <p>连接、模型与本地工作空间</p>
        </div>
      </div>
      <div className="tabs settings-tabs" role="tablist" aria-label="设置分类">
        {tabs.map(([key, name]) => (
          <button
            role="tab"
            id={`settings-tab-${key}`}
            aria-controls={`settings-panel-${key}`}
            tabIndex={tab === key ? 0 : -1}
            aria-selected={tab === key}
            key={key}
            className={tab === key ? "active" : ""}
            onClick={() => setTab(key)}
            onKeyDown={(event) => {
              if (
                !["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)
              )
                return;
              event.preventDefault();
              const current = tabs.findIndex(([id]) => id === key);
              const next =
                event.key === "Home"
                  ? 0
                  : event.key === "End"
                    ? tabs.length - 1
                    : (current +
                        (event.key === "ArrowRight" ? 1 : -1) +
                        tabs.length) %
                      tabs.length;
              setTab(tabs[next][0]);
              document.getElementById(`settings-tab-${tabs[next][0]}`)?.focus();
            }}
          >
            {name}
          </button>
        ))}
      </div>
      <div
        className="settings-panel"
        role="tabpanel"
        id={`settings-panel-${tab}`}
        aria-labelledby={`settings-tab-${tab}`}
      >
        {tab === "agents" && (
          <Agents
            data={data}
            refresh={refresh}
            notify={notify}
            search={search}
          />
        )}
        {tab === "knowledge" && (
          <KnowledgePage
            data={data}
            refresh={refresh}
            notify={notify}
            search={search}
          />
        )}
        {tab === "skills" && (
          <Skills
            data={data}
            refresh={refresh}
            notify={notify}
            search={search}
          />
        )}
        {tab === "mcp" && (
          <McpSettings
            notify={notify}
            workspaceKey={`${info.workspaceId}:${info.dataRoot}`}
          />
        )}
        {tab === "models" && (
          <>
            <div className="section-heading">
              <h2>供应商与模型</h2>
              <button
                className="primary"
                onClick={() => {
                  setSecret("");
                  setError("");
                  setProvider({
                    id: crypto.randomUUID(),
                    name: "",
                    baseUrl: "https://api.openai.com/v1",
                    protocol: "openai-completions",
                    enabled: true,
                  });
                }}
              >
                <Plus size={15} />
                添加供应商
              </button>
            </div>
            {data.providers.map((p) => (
              <section className="provider-section" key={p.id}>
                <div className="section-heading">
                  <div>
                    <h2>
                      <Server size={18} />
                      {p.name}
                      <span
                        className={"tag " + (p.enabled ? "done" : "closed")}
                      >
                        {p.enabled ? "启用" : "停用"}
                      </span>
                    </h2>
                    <p>
                      {p.baseUrl} · {p.protocol}
                    </p>
                  </div>
                  <div className="actions">
                    <button
                      disabled={busy}
                      onClick={() =>
                        run(() =>
                          command("provider_test", { providerId: p.id }),
                        )
                      }
                    >
                      <RefreshCw size={14} />
                      测试连接
                    </button>
                    <button
                      onClick={() => {
                        setProvider(p);
                        setSecret("");
                        setError("");
                      }}
                    >
                      <Settings2 size={14} />
                      配置
                    </button>
                    <button
                      onClick={() => {
                        setError("");
                        setModel({
                          id: crypto.randomUUID(),
                          providerId: p.id,
                          name: "",
                          remoteModelId: "",
                          capability: "chat",
                          enabled: true,
                        });
                      }}
                    >
                      <Plus size={14} />
                      模型
                    </button>
                  </div>
                </div>
                <div className="table-wrap">
                  <table>
                    <thead>
                      <tr>
                        <th>模型名称</th>
                        <th>模型 ID</th>
                        <th>能力</th>
                        <th>状态</th>
                        <th>操作</th>
                      </tr>
                    </thead>
                    <tbody>
                      {data.models
                        .filter((m) => m.providerId === p.id)
                        .map((m) => (
                          <tr key={m.id}>
                            <td>{m.name}</td>
                            <td>{m.remoteModelId}</td>
                            <td>
                              {
                                {
                                  chat: "文本",
                                  vision: "视觉",
                                  embedding: "向量",
                                }[m.capability]
                              }
                            </td>
                            <td>
                              <button
                                type="button"
                                className="model-status-switch"
                                role="switch"
                                aria-checked={m.enabled}
                                aria-label={`${m.name} 启用状态`}
                                disabled={busy || modelBusy}
                                onClick={() => void toggleModel(m)}
                              >
                                <span
                                  className="model-switch-track"
                                  aria-hidden="true"
                                >
                                  <span />
                                </span>
                                <span>{m.enabled ? "启用" : "禁用"}</span>
                              </button>
                            </td>
                            <td className="actions">
                              <button
                                disabled={busy}
                                onClick={() =>
                                  run(() =>
                                    command("provider_test", {
                                      providerId: p.id,
                                      modelId: m.id,
                                    }),
                                  )
                                }
                              >
                                测试
                              </button>
                              <button
                                className="icon-button"
                                title="编辑模型"
                                disabled={modelBusy}
                                onClick={() => {
                                  setError("");
                                  setModel(m);
                                }}
                              >
                                <Pencil size={15} />
                              </button>
                              <button
                                type="button"
                                className="icon-button danger"
                                title={`删除模型 ${m.name}`}
                                aria-label={`删除模型 ${m.name}`}
                                disabled={busy || modelBusy}
                                onClick={() => {
                                  setRemoveError("");
                                  setRemoveModel(m);
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
              </section>
            ))}
            {!data.providers.length && (
              <div className="empty">
                <Server />
                <p>尚未配置模型供应商</p>
              </div>
            )}
          </>
        )}
        {tab === "zentao" && (
          <>
            <div className="section-heading">
              <h2>禅道开源版 22.0</h2>
              <div className="actions">
                <button
                  disabled={busy || zentaoSaving || !zentao.revision}
                  onClick={() =>
                    run(() =>
                      command("zentao_sync", { connectionId: zentao.id }),
                    )
                  }
                >
                  <RefreshCw size={14} />
                  {busy ? "同步中…" : "同步项目与执行"}
                </button>
                <button
                  disabled={!zentao.revision || zentaoSaving}
                  onClick={() =>
                    openUrl(zentao.baseUrl).catch((e) => notify(String(e)))
                  }
                >
                  在禅道管理
                </button>
              </div>
            </div>
            <p className="muted">
                {!zentao.revision
                  ? "首次使用请登录并保存 · "
                  : zentao.rememberCredentials
                ? "已启用令牌失效自动登录 · "
                : "使用已保存令牌 · "}
              同步范围：我参与的项目、我负责的执行（含已关闭） ·{" "}
              {(zentao.syncIntervalMinutes ?? 5) === 0
                ? "自动同步已关闭"
                : `每 ${zentao.syncIntervalMinutes ?? 5} 分钟自动同步`}{" "}·{" "}
              {zentao.lastSync
                ? "最近同步 " + new Date(zentao.lastSync).toLocaleString()
                : "尚未同步"}{" "}
              ·{" "}
              {data.projects.filter((p) => p.connectionId === zentao.id).length}{" "}
              个项目
              {" · BUG 最近同步 "}
              {zentao.lastBugSync
                ? new Date(zentao.lastBugSync).toLocaleString()
                : "尚未同步"}
            </p>
            <ZentaoConnectionDialog
              inline
              key={`${info.workspaceId}:${zentao.id}:${zentaoFormVersion}`}
              connection={zentao}
              workspaceId={info.workspaceId}
              onClose={() => {}}
              onBusyChange={setZentaoSaving}
              onSaved={async () => {
                await refresh();
                setZentaoFormVersion((version) => version + 1);
                notify("禅道配置已保存");
              }}
            />
          </>
        )}
        {tab === "storage" && (
          <>
            <section>
              <h2>存储位置</h2>
              <div className="settings-row">
                <div>
                  <strong>当前数据目录</strong>
                  <p className="path-text">{info.dataRoot}</p>
                </div>
                <button
                  onClick={() =>
                    revealItemInDir(info.dataRoot).catch((e) =>
                      notify(String(e)),
                    )
                  }
                >
                  <FolderOpen size={15} />在 Finder 中打开
                </button>
              </div>
              <p className="muted">默认：{info.defaultRoot}</p>
              <div className="actions">
                <button
                  disabled={busy}
                  onClick={async () => {
                    const target = await open({ directory: true });
                    if (
                      typeof target === "string" &&
                      confirm(
                        "将工作数据复制到该目录并切换？目标必须为空，原目录会保留。",
                      )
                    )
                      await run(() => command("storage_move", { target }));
                  }}
                >
                  <FolderOpen size={15} />
                  选择目录并迁移
                </button>
                <button
                  disabled={busy || info.dataRoot === info.defaultRoot}
                  onClick={() => {
                    if (confirm("迁移回默认目录？存在旧数据时将拒绝覆盖。"))
                      void run(() =>
                        command("storage_move", { target: info.defaultRoot }),
                      );
                  }}
                >
                  恢复默认路径
                </button>
              </div>
            </section>
            <section>
              <h2>备份与恢复</h2>
              <p className="muted">
                备份包含数据库与文档原件，不包含系统钥匙串中的凭据。
              </p>
              <div className="actions">
                <button
                  disabled={busy}
                  onClick={async () => {
                    const destination = await save({
                      defaultPath: `workbench-${new Date().toISOString().slice(0, 10)}.zip`,
                      filters: [{ name: "工作台备份", extensions: ["zip"] }],
                    });
                    if (destination)
                      await run(() => command("data_backup", { destination }));
                  }}
                >
                  <Download size={15} />
                  导出完整备份
                </button>
                <button
                  disabled={busy}
                  onClick={async () => {
                    const archive = await open({
                      filters: [{ name: "工作台备份", extensions: ["zip"] }],
                    });
                    if (typeof archive !== "string") return;
                    const target = await open({
                      directory: true,
                      title: "选择空目录用于恢复",
                    });
                    if (
                      typeof target === "string" &&
                      confirm(
                        "验证备份并恢复到所选空目录？验证成功后切换，当前数据保留。",
                      )
                    )
                      await run(() =>
                        command("data_restore", { archive, target }),
                      );
                  }}
                >
                  <Upload size={15} />
                  恢复备份
                </button>
              </div>
              {busy && <p role="status">正在处理数据，请勿退出应用。</p>}
            </section>
          </>
        )}
        {tab === "mail" && <MailSettings />}
        {tab === "ding-calendar" && (
          <DingCalendarSettings key={`${info.workspaceId}:${info.dataRoot}`} />
        )}
        {tab === "general" && <GeneralSettings />}
        {tab === "about" && (
          <section>
            <h2>栖点 · Perch</h2>
            <p>版本 {info.version}</p>
            <p>Tauri · React · SQLite</p>
            <p className="muted">个人任务、项目、日历与 AI 工作空间</p>
          </section>
        )}
      </div>
      {catalogProvider && (
        <ProviderModelsDialog
          key={catalogProvider.id}
          provider={catalogProvider}
          models={data.models}
          onClose={() => setCatalogProvider(null)}
          onSaved={async ({ added, skipped }) => {
            await refresh();
            notify(
              `已添加 ${added} 个模型${skipped ? `，跳过 ${skipped} 个已有模型` : ""}`,
            );
            setProvider(null);
          }}
        />
      )}
      {provider && !catalogProvider && (
        <Modal
          title="供应商配置"
          busy={providerBusy}
          onClose={() => setProvider(null)}
        >
          <form onSubmit={storeProvider}>
            <fieldset
              className="form-grid provider-form-fields"
              disabled={providerBusy}
            >
              <label>
                名称
                <input
                  required
                  value={provider.name}
                  onChange={(e) =>
                    setProvider({ ...provider, name: e.target.value })
                  }
                />
              </label>
              <label>
                API 根地址
                <input
                  required
                  type="url"
                  value={provider.baseUrl}
                  onChange={(e) =>
                    setProvider({ ...provider, baseUrl: e.target.value })
                  }
                />
              </label>
              <label>
                协议
                <GlassSelect
                  aria-label="供应商协议"
                  disabled={providerBusy}
                  value={provider.protocol}
                  onValueChange={(protocol) =>
                    setProvider({
                      ...provider,
                      protocol: protocol as Provider["protocol"],
                    })
                  }
                  options={[
                    {
                      value: "openai-completions",
                      label: "OpenAI Chat Completions",
                    },
                    { value: "openai-responses", label: "OpenAI Responses" },
                    {
                      value: "anthropic-messages",
                      label: "Anthropic Messages",
                    },
                  ]}
                />
              </label>
              <label>
                API Key
                <input
                  type="password"
                  autoComplete="new-password"
                  value={secret}
                  placeholder="留空保留现有密钥"
                  onChange={(e) => setSecret(e.target.value)}
                />
              </label>
              <label className="check-label">
                <input
                  type="checkbox"
                  checked={provider.enabled}
                  onChange={(e) =>
                    setProvider({ ...provider, enabled: e.target.checked })
                  }
                />
                启用供应商
              </label>
              <p className="muted">
                拉取前会保存当前配置；API Key
                留空可保留原密钥，本地免密服务可直接留空。
              </p>
              {error && (
                <p className="error" role="alert">
                  {error}
                </p>
              )}
              <footer>
                {provider.revision && (
                  <button
                    type="button"
                    className="danger"
                    onClick={async () => {
                      if (confirm("删除供应商？有关联时会禁止删除。"))
                        try {
                          await api.remove(
                            "provider",
                            provider.id,
                            provider.revision,
                          );
                          await refresh();
                          setProvider(null);
                        } catch (e) {
                          setError((e as Error).message);
                        }
                    }}
                  >
                    <Trash2 size={15} />
                    删除
                  </button>
                )}
                <button type="submit" value="models">
                  <Download size={15} />
                  {providerBusy ? "正在保存…" : "保存并拉取模型"}
                </button>
                <button type="submit" className="primary">
                  保存
                </button>
              </footer>
            </fieldset>
          </form>
        </Modal>
      )}
      {model && (
        <Modal title="模型配置" onClose={() => setModel(null)}>
          <form
            className="form-grid"
            onSubmit={async (e) => {
              e.preventDefault();
              try {
                await api.save("model", model);
                await refresh();
                setModel(null);
              } catch (e) {
                setError((e as Error).message);
              }
            }}
          >
            <label>
              显示名称
              <input
                required
                value={model.name}
                onChange={(e) => setModel({ ...model, name: e.target.value })}
              />
            </label>
            <label>
              远端模型 ID
              <input
                required
                value={model.remoteModelId}
                onChange={(e) =>
                  setModel({ ...model, remoteModelId: e.target.value })
                }
              />
            </label>
            <div>
              <p>模型类型</p>
              <ModelCapability
                value={model.capability}
                onValueChange={(capability) =>
                  setModel({
                    ...model,
                    capability: capability as Model["capability"],
                  })
                }
              />
            </div>
            <label className="check-label">
              <input
                type="checkbox"
                checked={model.enabled}
                onChange={(e) =>
                  setModel({ ...model, enabled: e.target.checked })
                }
              />
              启用模型
            </label>
            {error && <p className="error">{error}</p>}
            <footer>
              {model.revision && (
                <button
                  type="button"
                  className="danger"
                  onClick={async () => {
                    if (confirm("删除此模型？"))
                      try {
                        await api.remove("model", model.id, model.revision);
                        await refresh();
                        setModel(null);
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
      {removeModel && (
        <Modal
          title="删除模型"
          busy={modelBusy}
          onClose={() => setRemoveModel(null)}
        >
          <p>确定删除“{removeModel.name}”？此操作仅移除本地模型配置。</p>
          <p className="muted">
            如果模型正在被 Agent
            或知识库引用，请先更换它们使用的模型；也可以仅禁用此模型。
          </p>
          {removeError && (
            <p className="error" role="alert">
              {removeError}
            </p>
          )}
          <footer>
            <button
              type="button"
              disabled={modelBusy}
              onClick={() => setRemoveModel(null)}
            >
              取消
            </button>
            <button
              type="button"
              className="danger"
              disabled={modelBusy}
              onClick={() => void confirmModelRemoval()}
            >
              {modelBusy ? "删除中…" : "确认删除"}
            </button>
          </footer>
        </Modal>
      )}
    </>
  );
}

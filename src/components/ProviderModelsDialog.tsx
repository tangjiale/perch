import { useEffect, useRef, useState } from "react";
import { Download, RefreshCw, Search } from "lucide-react";
import { command } from "../lib/api";
import type { Model, Provider } from "../lib/types";
import Modal from "./Modal";
import ModelCapability from "./ModelCapability";
import "./provider-models.css";

interface Catalog {
  providerId: string;
  providerRevision: number;
  workspaceId: string;
  generation: string;
  models: { id: string; name: string }[];
}
interface ImportResult {
  added: number;
  skipped: number;
}

export default function ProviderModelsDialog({
  provider,
  models,
  onClose,
  onSaved,
}: {
  provider: Provider;
  models: Model[];
  onClose: () => void;
  onSaved: (result: ImportResult) => Promise<unknown>;
}) {
  const [catalog, setCatalog] = useState<Catalog | null>(null);
  const [selected, setSelected] = useState<Map<string, Model["capability"]>>(
    new Map(),
  );
  const [query, setQuery] = useState("");
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");
  const [saved, setSaved] = useState<ImportResult | null>(null);
  const request = useRef(0);
  const inFlight = useRef(false);
  const existing = new Set(
    models
      .filter((m) => m.providerId === provider.id)
      .map((m) => m.remoteModelId),
  );
  const visible = (catalog?.models ?? []).filter((m) =>
    `${m.id} ${m.name}`.toLowerCase().includes(query.trim().toLowerCase()),
  );
  const available = visible.filter((m) => !existing.has(m.id));

  async function load() {
    const id = ++request.current;
    setLoading(true);
    setError("");
    try {
      const result = await command<Catalog>("provider_models", {
        providerId: provider.id,
      });
      if (request.current !== id) return;
      setCatalog(result);
      setSelected(new Map());
    } catch (e) {
      if (request.current === id) {
        setError((e as Error).message);
        setCatalog(null);
      }
    } finally {
      if (request.current === id) setLoading(false);
    }
  }
  useEffect(() => {
    void load();
    return () => {
      request.current++;
    };
    // 弹窗以供应商 ID 为 key；卸载时丢弃迟到的目录结果。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  function toggle(id: string) {
    setSelected((current) => {
      const next = new Map(current);
      if (next.has(id)) next.delete(id);
      else next.set(id, "chat");
      return next;
    });
  }
  async function save() {
    if (inFlight.current || !catalog || (!saved && !selected.size)) return;
    inFlight.current = true;
    setSaving(true);
    setError("");
    try {
      const result =
        saved ??
        (await command<ImportResult>("provider_models_import", {
          catalog: {
            providerId: catalog.providerId,
            providerRevision: catalog.providerRevision,
            workspaceId: catalog.workspaceId,
            generation: catalog.generation,
          },
          models: catalog.models
            .filter((m) => selected.has(m.id))
            .map((m) => ({
              id: m.id,
              name: m.name,
              capability: selected.get(m.id),
            })),
        }));
      // 已提交后仅重试刷新，避免刷新失败触发第二次写入。
      setSaved(result);
      await onSaved(result);
      onClose();
    } catch (e) {
      setError((e as Error).message);
    } finally {
      inFlight.current = false;
      setSaving(false);
    }
  }

  return (
    <Modal title="选择模型" wide busy={saving} onClose={onClose}>
      <div className="model-catalog">
        <div className="model-catalog-intro">
          <div>
            <strong>{provider.name}</strong>
            <p className="muted">
              从供应商目录勾选需要的模型，添加后即可配置使用。
            </p>
          </div>
          <button
            type="button"
            disabled={loading || saving || !!saved}
            onClick={() => void load()}
          >
            <RefreshCw size={14} />
            重新拉取
          </button>
        </div>
        <p className="model-catalog-hint">
          模型类型：文本用于文字聊天，视觉支持图片聊天，向量用于知识库检索。目录通常不提供能力信息，默认选择文本，请按模型实际能力调整。
        </p>
        {error && (
          <p className="error" role="alert">
            {saved ? "模型已添加，列表刷新失败：" : ""}
            {error}
          </p>
        )}
        {loading ? (
          <div className="model-catalog-empty" role="status">
            正在拉取模型列表…
          </div>
        ) : catalog ? (
          <>
            <label className="model-catalog-search">
              <Search size={16} />
              <input
                aria-label="搜索模型"
                placeholder="搜索模型名称或 ID"
                value={query}
                disabled={saving || !!saved}
                onChange={(e) => setQuery(e.target.value)}
              />
            </label>
            <div className="model-catalog-toolbar">
              <span className="muted">
                共 {catalog.models.length} 个 · 已选 {selected.size} 个
              </span>
              <div>
                <button
                  type="button"
                  disabled={!available.length || saving || !!saved}
                  onClick={() =>
                    setSelected((current) => {
                      const next = new Map(current);
                      available.forEach((m) => {
                        if (!next.has(m.id)) next.set(m.id, "chat");
                      });
                      return next;
                    })
                  }
                >
                  全选当前结果
                </button>
                <button
                  type="button"
                  disabled={!selected.size || saving || !!saved}
                  onClick={() => setSelected(new Map())}
                >
                  清空选择
                </button>
              </div>
            </div>
            <div className="model-catalog-list" aria-label="可用模型">
              {visible.map((m) => (
                <div
                  className="model-catalog-row"
                  key={m.id}
                  data-selected={selected.has(m.id) || undefined}
                >
                  <label className="model-catalog-choice">
                    <input
                      type="checkbox"
                      aria-label={`选择 ${m.id}`}
                      checked={existing.has(m.id) || selected.has(m.id)}
                      disabled={existing.has(m.id) || saving || !!saved}
                      onChange={() => toggle(m.id)}
                    />
                    <span>
                      <strong>{m.name}</strong>
                      <small>{m.id}</small>
                    </span>
                  </label>
                  {existing.has(m.id) ? (
                    <span className="model-catalog-badge">已添加</span>
                  ) : (
                    <ModelCapability
                      label={`${m.id} 模型类型`}
                      value={selected.get(m.id) ?? "chat"}
                      disabled={!selected.has(m.id) || saving || !!saved}
                      onValueChange={(value) =>
                        setSelected((current) =>
                          new Map(current).set(
                            m.id,
                            value as Model["capability"],
                          ),
                        )
                      }
                    />
                  )}
                </div>
              ))}
              {!visible.length && (
                <div className="model-catalog-empty">
                  {catalog.models.length
                    ? "没有匹配的模型，试试其他关键词。"
                    : "供应商返回了空目录，可返回配置后手动添加模型。"}
                </div>
              )}
            </div>
          </>
        ) : (
          <div className="model-catalog-empty">
            未能获取模型目录。请检查地址、协议和密钥后重试；不支持目录接口的服务可手动添加模型。
          </div>
        )}
        <footer>
          <button type="button" disabled={saving} onClick={onClose}>
            返回配置
          </button>
          <button
            type="button"
            className="primary"
            disabled={
              loading || saving || (!saved && !selected.size) || !catalog
            }
            onClick={() => void save()}
          >
            <Download size={15} />
            {saving
              ? "正在处理…"
              : saved
                ? "重试刷新"
                : `添加所选模型${selected.size ? `（${selected.size}）` : ""}`}
          </button>
        </footer>
      </div>
    </Modal>
  );
}

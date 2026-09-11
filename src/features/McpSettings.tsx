import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import {
  Plus,
  Pencil,
  Trash2,
  Plug,
  Terminal,
  Globe,
  ShieldCheck,
  RefreshCw,
} from "lucide-react";
import { command, native } from "../lib/api";
import Modal from "../components/Modal";
import GlassSelect from "../components/GlassSelect";

interface McpConnection {
  id: string;
  revision?: number;
  name: string;
  transport: "stdio" | "streamable-http";
  url: string;
  command: string;
  args: string[];
  enabled: boolean;
  hasSecrets?: boolean;
}

export default function McpSettings({
  notify,
  workspaceKey,
}: {
  notify: (message: string) => void;
  workspaceKey: string;
}) {
  const {
    data: connections = [],
    error: loadError,
    isPending,
    refetch,
  } = useQuery({
    queryKey: ["mcp", workspaceKey],
    queryFn: () =>
      native ? command<McpConnection[]>("mcp_list") : Promise.resolve([]),
  });
  const [draft, setDraft] = useState<McpConnection | null>(null);
  const [args, setArgs] = useState("");
  const [secret, setSecret] = useState("");
  const [replaceSecret, setReplaceSecret] = useState(false);
  const [remove, setRemove] = useState<McpConnection | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  function edit(value: McpConnection) {
    setDraft(value);
    setArgs(value.args.join("\n"));
    setSecret("");
    setReplaceSecret(false);
    setError("");
  }
  async function save(event: React.FormEvent) {
    event.preventDefault();
    if (!draft || busy) return;
    setBusy(true);
    setError("");
    try {
      let secretJson: string | null = null;
      if (replaceSecret || secret.trim()) {
        if (secret.trim()) {
          const parsed: unknown = JSON.parse(secret);
          if (
            !parsed ||
            typeof parsed !== "object" ||
            Array.isArray(parsed) ||
            Object.values(parsed).some((value) => typeof value !== "string")
          )
            throw Error("请输入名称和值均为字符串的 JSON 对象");
          secretJson = JSON.stringify({
            [draft.transport === "stdio" ? "env" : "headers"]: parsed,
          });
        } else secretJson = "";
      }
      await command("mcp_save", {
        value: {
          ...draft,
          name: draft.name.trim(),
          args: args.split("\n").filter((line) => line.length > 0),
        },
        secretJson,
      });
      await refetch();
      setDraft(null);
      setSecret("");
      notify("MCP 配置已保存");
    } catch (reason) {
      setError((reason as Error).message);
    } finally {
      setBusy(false);
    }
  }
  return (
    <>
      <div className="section-heading">
        <h2>MCP 连接</h2>
        <button
          className="primary"
          onClick={() =>
            edit({
              id: crypto.randomUUID(),
              name: "",
              transport: "streamable-http",
              url: "",
              command: "",
              args: [],
              enabled: true,
            })
          }
        >
          <Plus size={16} />
          添加连接
        </button>
      </div>
      {isPending && <p role="status">正在读取连接配置…</p>}
      {loadError && (
        <p className="error" role="alert">
          {loadError.message}
          <button
            className="icon-button"
            title="重新读取"
            onClick={() => void refetch()}
          >
            <RefreshCw size={15} />
          </button>
        </p>
      )}
      {!isPending && !loadError && !connections.length && (
        <div className="empty">
          <Plug size={30} />
          <p>暂无 MCP 连接</p>
        </div>
      )}
      {!!connections.length && (
        <div className="table-wrap">
          <table className="mcp-table">
            <thead>
              <tr>
                <th>连接</th>
                <th>传输方式</th>
                <th>配置状态</th>
                <th>认证</th>
                <th>
                  <span className="sr-only">操作</span>
                </th>
              </tr>
            </thead>
            <tbody>
              {connections.map((connection) => (
                <tr key={connection.id}>
                  <td>
                    <strong>{connection.name}</strong>
                    <small
                      className="mcp-endpoint"
                      title={
                        connection.transport === "stdio"
                          ? [connection.command, ...connection.args].join(" ")
                          : connection.url
                      }
                    >
                      {connection.transport === "stdio"
                        ? connection.command
                        : connection.url}
                    </small>
                  </td>
                  <td>
                    {connection.transport === "stdio" ? (
                      <>
                        <Terminal size={14} /> STDIO
                      </>
                    ) : (
                      <>
                        <Globe size={14} /> Streamable HTTP
                      </>
                    )}
                  </td>
                  <td>
                    <span className={`tag ${connection.enabled ? "done" : ""}`}>
                      {connection.enabled ? "已启用" : "已停用"}
                    </span>
                  </td>
                  <td>
                    {connection.hasSecrets ? (
                      <span title="凭据存储在系统钥匙串">
                        <ShieldCheck size={14} /> 已配置
                      </span>
                    ) : (
                      <span className="muted">未配置</span>
                    )}
                  </td>
                  <td>
                    <div className="row-actions">
                      <button
                        className="icon-button"
                        title={`编辑 ${connection.name}`}
                        onClick={() => edit(connection)}
                      >
                        <Pencil size={15} />
                      </button>
                      <button
                        className="icon-button"
                        title={`删除 ${connection.name}`}
                        onClick={() => {
                          setError("");
                          setRemove(connection);
                        }}
                      >
                        <Trash2 size={15} />
                      </button>
                    </div>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      {draft && (
        <Modal
          title={draft.revision ? "编辑 MCP 连接" : "添加 MCP 连接"}
          onClose={() => {
            if (!busy) {
              setDraft(null);
              setSecret("");
            }
          }}
        >
          <form className="form-grid" onSubmit={save}>
            <label>
              名称
              <input
                required
                maxLength={128}
                value={draft.name}
                onChange={(e) => setDraft({ ...draft, name: e.target.value })}
              />
            </label>
            <label>
              传输方式
              <GlassSelect
                aria-label="MCP 传输方式"
                value={draft.transport}
                onValueChange={(transport) => {
                  setDraft({
                    ...draft,
                    transport: transport as McpConnection["transport"],
                  });
                  setSecret("");
                  setReplaceSecret(false);
                }}
                options={[
                  { value: "streamable-http", label: "Streamable HTTP" },
                  { value: "stdio", label: "STDIO" },
                ]}
              />
            </label>
            {draft.transport === "streamable-http" ? (
              <label>
                服务地址
                <input
                  type="url"
                  required
                  placeholder="https://mcp.example.com/mcp"
                  value={draft.url}
                  onChange={(e) => setDraft({ ...draft, url: e.target.value })}
                />
              </label>
            ) : (
              <>
                <label>
                  启动命令
                  <input
                    required
                    placeholder="/usr/local/bin/npx"
                    value={draft.command}
                    onChange={(e) =>
                      setDraft({ ...draft, command: e.target.value })
                    }
                  />
                </label>
                <label>
                  启动参数（每行一个）
                  <textarea
                    rows={3}
                    value={args}
                    onChange={(e) => setArgs(e.target.value)}
                    placeholder={
                      "-y\n@modelcontextprotocol/server-filesystem\n/Users/your-name/Documents"
                    }
                  />
                </label>
              </>
            )}
            {draft.hasSecrets && (
              <label className="check-label">
                <input
                  type="checkbox"
                  checked={replaceSecret}
                  onChange={(e) => {
                    setReplaceSecret(e.target.checked);
                    setSecret("");
                  }}
                />
                替换已保存的认证配置
              </label>
            )}
            {(!draft.hasSecrets || replaceSecret) && (
              <label>
                {draft.transport === "stdio"
                  ? "环境变量（JSON）"
                  : "请求头（JSON）"}
                <textarea
                  rows={4}
                  autoComplete="off"
                  spellCheck={false}
                  value={secret}
                  onChange={(e) => setSecret(e.target.value)}
                  placeholder={
                    draft.transport === "stdio"
                      ? '{"API_KEY": "..."}'
                      : '{"Authorization": "Bearer ..."}'
                  }
                />
                {replaceSecret && <small>留空将清除已保存的认证配置</small>}
              </label>
            )}
            <label className="check-label">
              <input
                type="checkbox"
                checked={draft.enabled}
                onChange={(e) =>
                  setDraft({ ...draft, enabled: e.target.checked })
                }
              />
              启用配置
            </label>
            {error && (
              <p className="error" role="alert">
                {error}
              </p>
            )}
            <footer>
              <button
                type="button"
                disabled={busy}
                onClick={() => {
                  setDraft(null);
                  setSecret("");
                }}
              >
                取消
              </button>
              <button className="primary" disabled={busy}>
                {busy ? "正在保存…" : "保存配置"}
              </button>
            </footer>
          </form>
        </Modal>
      )}
      {remove && (
        <Modal
          title="删除 MCP 连接"
          onClose={() => {
            if (!busy) setRemove(null);
          }}
        >
          <p>删除“{remove.name}”及其保存的认证配置？</p>
          {error && (
            <p className="error" role="alert">
              {error}
            </p>
          )}
          <div className="modal-actions">
            <button disabled={busy} onClick={() => setRemove(null)}>
              取消
            </button>
            <button
              className="danger"
              disabled={busy}
              onClick={async () => {
                setBusy(true);
                try {
                  await command("mcp_delete", {
                    id: remove.id,
                    revision: remove.revision,
                  });
                  await refetch();
                  setRemove(null);
                  notify("MCP 配置已删除");
                } catch (reason) {
                  setError((reason as Error).message);
                } finally {
                  setBusy(false);
                }
              }}
            >
              <Trash2 size={14} />
              {busy ? "正在删除…" : "删除连接"}
            </button>
          </div>
        </Modal>
      )}
    </>
  );
}

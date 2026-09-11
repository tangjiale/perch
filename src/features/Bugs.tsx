import { useRef, useState, type FormEvent } from "react";
import {
  Bug,
  ArrowDown,
  ArrowUp,
  ArrowUpDown,
  CheckCircle2,
  ExternalLink,
  RefreshCw,
  Search,
  Settings2,
  XCircle,
} from "lucide-react";
import { useNavigate, useSearchParams } from "react-router-dom";
import { DateTime } from "luxon";
import { openUrl } from "@tauri-apps/plugin-opener";
import DOMPurify from "dompurify";
import GlassSelect from "../components/GlassSelect";
import Modal from "../components/Modal";
import { command } from "../lib/api";
import type { PageProps, ZentaoBug } from "../lib/types";
import { bugProjectKey, buildBugProjectIndex } from "../lib/bug-projects";
import { sortBugs, type BugSortField, type BugSortDirection } from "../lib/bug-sort";
import "./bugs.css";

const statusNames = { active: "待解决", resolved: "已解决", closed: "已关闭" };
const resolutionOptions = [
  { value: "fixed", label: "已修复" },
  { value: "duplicate", label: "重复 BUG" },
  { value: "bydesign", label: "设计如此" },
  { value: "notrepro", label: "无法重现" },
  { value: "postponed", label: "延期处理" },
  { value: "willnotfix", label: "不予解决" },
];
const levels = [1, 2, 3, 4].map((value) => ({
  value: String(value),
  label: String(value),
}));
type Action = "resolve" | "close" | "activate";
const actionNames: Record<Action, string> = {
  resolve: "解决 BUG",
  close: "关闭 BUG",
  activate: "激活 BUG",
};

function dateLabel(value?: string) {
  if (!value || value.startsWith("0000")) return "未设置";
  const date = DateTime.fromISO(value.replace(" ", "T"));
  return date.isValid
    ? date.toFormat(value.length === 10 ? "yyyy-MM-dd" : "yyyy-MM-dd HH:mm")
    : "未设置";
}

// 只展示文本排版；禁止远端图片、链接和嵌入内容在详情中发起请求。
function safeSteps(value: string) {
  return DOMPurify.sanitize(value, {
    ALLOWED_TAGS: [
      "p",
      "br",
      "b",
      "strong",
      "em",
      "i",
      "u",
      "s",
      "ul",
      "ol",
      "li",
      "pre",
      "code",
      "blockquote",
      "table",
      "thead",
      "tbody",
      "tr",
      "td",
      "th",
      "h1",
      "h2",
      "h3",
      "h4",
    ],
    ALLOWED_ATTR: [],
  });
}

export default function Bugs({
  data,
  refresh,
  notify,
  search = "",
}: PageProps & { search?: string }) {
  const navigate = useNavigate();
  const [params, setParams] = useSearchParams();
  const filter = ["all", "active", "resolved", "closed"].includes(
    params.get("status") || "",
  )
    ? params.get("status")!
    : "active";
  const setFilter = (status: string) => {
    const next = new URLSearchParams(params);
    next.set("status", status);
    setParams(next);
  };
  const [severity, setSeverity] = useState("all");
  const [query, setQuery] = useState("");
  const [page, setPage] = useState(0);
  const [sortField, setSortField] = useState<BugSortField>("openedDate");
  const [sortDirection, setSortDirection] = useState<BugSortDirection>("desc");
  const [edit, setEdit] = useState<ZentaoBug | null>(null);
  const [editing, setEditing] = useState(false);
  const [action, setAction] = useState<Action | null>(null);
  const [resolution, setResolution] = useState("fixed");
  const [build, setBuild] = useState("trunk");
  const [duplicate, setDuplicate] = useState("");
  const [comment, setComment] = useState("");
  const [assignee, setAssignee] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const lock = useRef(false);
  const connection = data.connections[0];
  const allBugs = data.bugs ?? [];
  const projectFilter = params.get("project") || "all";
  const projectGroups = buildBugProjectIndex(data.projects, allBugs);
  const projectBugs =
    projectFilter === "all"
      ? allBugs
      : allBugs.filter(
          (bug) =>
            bugProjectKey(bug.connectionId, bug.projectId) === projectFilter,
        );
  const words = `${search} ${query}`
    .trim()
    .toLocaleLowerCase()
    .split(/\s+/)
    .filter(Boolean);
  const filtered = projectBugs.filter(
    (bug) =>
      (filter === "all" || bug.status === filter) &&
      (severity === "all" || bug.severity === Number(severity)) &&
      words.every((word) =>
        `${bug.remoteId} ${bug.title} ${bug.productName ?? ""} ${bug.projectName ?? ""} ${bug.assignedToName ?? ""} ${bug.assignedTo}`
          .toLocaleLowerCase()
          .includes(word),
      ),
  );
  const pages = Math.max(1, Math.ceil(filtered.length / 50));
  const currentPage = Math.min(page, pages - 1);
  const sorted = sortBugs(filtered, sortField, sortDirection);
  function sortButton(field: BugSortField, label: string) {
    const active = sortField === field;
    const Icon = active ? (sortDirection === "asc" ? ArrowUp : ArrowDown) : ArrowUpDown;
    return (
      <button type="button" className="bugs-sort-button" data-active={active}
        aria-label={`${label}，${active ? `当前${sortDirection === "asc" ? "升序" : "倒序"}，` : ""}点击${active && sortDirection === "asc" ? "倒序" : "升序"}排序`}
        onClick={() => {
          setSortDirection(active && sortDirection === "asc" ? "desc" : "asc");
          setSortField(field);
          setPage(0);
        }}>
        {label}<Icon size={13} aria-hidden="true" />
      </button>
    );
  }
  const columnOrder = (fields: BugSortField[]): "ascending" | "descending" | "none" =>
    fields.includes(sortField) ? (sortDirection === "asc" ? "ascending" : "descending") : "none";

  async function run(
    operation: () => Promise<unknown>,
    message: string,
    mutation = false,
  ) {
    if (lock.current) return;
    lock.current = true;
    setBusy(true);
    setError("");
    try {
      const result = await operation();
      if (mutation) {
        setEdit(null);
        setAction(null);
      }
      try {
        await refresh();
      } catch {
        setError(
          "禅道操作已成功，但本地页面刷新失败。请刷新页面核对结果，不要重复提交。",
        );
        return;
      }
      notify(typeof result === "string" ? result : message);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
      lock.current = false;
    }
  }

  function openDetail(bug: ZentaoBug) {
    setEdit({ ...bug });
    setEditing(false);
    setAction(null);
    setError("");
  }
  function beginAction(next: Action) {
    setAction(next);
    setResolution("fixed");
    setBuild(next === "activate" ? "" : "trunk");
    setDuplicate("");
    setComment("");
    setAssignee(edit?.assignedTo || edit?.lastAssignedTo || "");
    setError("");
  }
  async function transition(event: FormEvent) {
    event.preventDefault();
    if (!edit || !action) return;
    const payload: Record<string, unknown> = {
      comment,
      assignedTo: assignee.trim(),
    };
    if (action === "resolve") {
      payload.resolution = resolution;
      if (resolution === "fixed") payload.resolvedBuild = build.trim();
      if (resolution === "duplicate") payload.duplicateBug = Number(duplicate);
    }
    if (action === "activate" && build.trim())
      payload.openedBuild = build
        .split(",")
        .map((value) => value.trim())
        .filter(Boolean);
    await run(
      () =>
        command<ZentaoBug>("zentao_bug_transition", {
          id: edit.id,
          revision: edit.revision,
          action,
          payload,
        }),
      `${actionNames[action]}成功`,
      true,
    );
  }
  async function openRemote() {
    if (!edit) return;
    const source = data.connections.find(
      (item) => item.id === edit.connectionId,
    );
    if (!source) {
      setError("禅道连接已不存在，请先检查连接设置。");
      return;
    }
    try {
      const url = new URL("index.php", `${source.baseUrl.replace(/\/$/, "")}/`);
      if (!["http:", "https:"].includes(url.protocol))
        throw new Error("禅道地址必须使用 HTTP 或 HTTPS");
      url.search = new URLSearchParams({
        m: "bug",
        f: "view",
        bugID: edit.remoteId,
      }).toString();
      await openUrl(url.toString());
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }

  return (
    <>
      <div className="page-heading">
        <div>
          <h1>BUG 修复</h1>
          <p>禅道中指派给我的 BUG，查看进展并同步处理结果</p>
        </div>
        <div className="bugs-actions">
          <button
            onClick={() => navigate("/settings?tab=zentao")}
            disabled={busy}
          >
            <Settings2 size={16} />
            禅道设置
          </button>
          <button
            className="primary"
            disabled={busy || !connection?.enabled}
            onClick={() =>
              void run(
                () =>
                  command<string>("zentao_bugs_sync", {
                    connectionId: connection!.id,
                  }),
                "BUG 同步完成",
              )
            }
          >
            <RefreshCw size={16} />
            {busy ? "正在处理…" : "同步我的 BUG"}
          </button>
        </div>
      </div>
      {!edit && error && (
        <div className="error-panel" role="alert">
          {error}
        </div>
      )}
      {!connection ? (
        <div className="empty">
          <Bug size={32} />
          <h3>先连接禅道</h3>
          <p>配置连接后，可以同步所有当前指派给你的 BUG。</p>
          <button onClick={() => navigate("/settings?tab=zentao")}>
            配置禅道连接
          </button>
        </div>
      ) : (
        <>
          <div className="bugs-toolbar">
            <div
              className="bugs-status-filters"
              role="group"
              aria-label="BUG 状态筛选"
            >
              {[
                { value: "all", label: "全部" },
                ...Object.entries(statusNames).map(([value, label]) => ({
                  value,
                  label,
                })),
              ].map((item) => (
                <button
                  key={item.value}
                  aria-pressed={filter === item.value}
                  onClick={() => {
                    setFilter(item.value);
                    setPage(0);
                  }}
                >
                  {item.label}
                  <span>
                    {item.value === "all"
                      ? projectBugs.length
                      : projectBugs.filter((bug) => bug.status === item.value)
                          .length}
                  </span>
                </button>
              ))}
            </div>
            <label className="bugs-search">
              <GlassSelect
                aria-label="筛选 BUG 项目"
                value={projectFilter}
                onValueChange={(value) => {
                  const next = new URLSearchParams(params);
                  if (value === "all") next.delete("project");
                  else next.set("project", value);
                  setParams(next);
                  setPage(0);
                }}
                options={[
                  { value: "all", label: "全部项目" },
                  ...Array.from(projectGroups, ([value, group]) => ({
                    value,
                    label: `${group.label}（${group.total}）`,
                  })),
                  ...(projectFilter !== "all" &&
                  !projectGroups.has(projectFilter)
                    ? [{ value: projectFilter, label: "项目已不可用" }]
                    : []),
                ]}
              />
              <GlassSelect
                aria-label="筛选 BUG 严重程度"
                value={severity}
                onValueChange={(value) => {
                  setSeverity(value);
                  setPage(0);
                }}
                options={[
                  { value: "all", label: "全部严重程度" },
                  ...levels.map((level) => ({
                    ...level,
                    label: `严重程度 ${level.value}`,
                  })),
                ]}
              />
              <Search size={16} />
              <input
                aria-label="搜索 BUG"
                placeholder="搜索编号、标题、产品或项目"
                value={query}
                onChange={(event) => {
                  setQuery(event.target.value);
                  setPage(0);
                }}
              />
            </label>
          </div>
          {!connection.enabled && (
            <p className="error" role="status">
              禅道连接已停用，请在禅道设置中启用后同步。
            </p>
          )}
          <p className="bugs-scope">
            {connection.lastBugSync
              ? `最近同步：${new Date(connection.lastBugSync).toLocaleString("zh-CN")}。`
              : "尚未同步。"}
            显示最近同步时指派给我的全部状态
            BUG。转派给他人后将移出此列表；修改与状态操作会更新禅道。
          </p>
          {filtered.length === 0 ? (
            <div className="empty">
              <Bug size={32} />
              <h3>
                {allBugs.length ? "没有符合条件的 BUG" : "暂无指派给我的 BUG"}
              </h3>
              <p>
                {allBugs.length
                  ? "试试切换项目、状态、严重程度或调整关键词。"
                  : "点击“同步我的 BUG”，获取禅道中的最新指派。"}
              </p>
            </div>
          ) : (
            <>
              <div className="bugs-table-wrap">
                <table className="bugs-table">
                  <thead>
                    <tr>
                      <th scope="col">BUG</th>
                      <th scope="col" aria-sort={columnOrder(["status"])}>{sortButton("status", "状态")}</th>
                      <th scope="col" aria-sort={columnOrder(["priority", "severity"])}>{sortButton("priority", "优先级")} / {sortButton("severity", "严重程度")}</th>
                      <th scope="col">产品 / 项目</th>
                      <th scope="col">指派给</th>
                      <th scope="col" aria-sort={columnOrder(["openedDate", "deadline"])}>{sortButton("openedDate", "创建")} / {sortButton("deadline", "截止日期")}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {sorted
                      .slice(currentPage * 50, (currentPage + 1) * 50)
                      .map((bug) => (
                        <tr key={bug.id}>
                          <td>
                            <button
                              className="bugs-title"
                              disabled={busy}
                              onClick={() => openDetail(bug)}
                            >
                              <span>#{bug.remoteId}</span>
                              <strong>{bug.title}</strong>
                            </button>
                          </td>
                          <td>
                            <span
                              className={`bug-status bug-status-${bug.status}`}
                            >
                              {statusNames[bug.status] ?? bug.status}
                            </span>
                          </td>
                          <td>
                            <span
                              className={
                                bug.priority === 1 ? "bugs-urgent" : ""
                              }
                            >
                              P{bug.priority}
                            </span>
                            <small>严重程度 {bug.severity}</small>
                          </td>
                          <td>
                            {bug.productName ||
                              (bug.productId
                                ? `产品 #${bug.productId}`
                                : "未关联产品")}
                            <small>
                              {bug.projectName ||
                                (bug.projectId
                                  ? `项目 #${bug.projectId}`
                                  : "未关联项目")}
                            </small>
                          </td>
                          <td>
                            {bug.assignedToName || bug.assignedTo || "未指派"}
                            <small>
                              {bug.assignedToName ? bug.assignedTo : ""}
                            </small>
                          </td>
                          <td>
                            <time>{dateLabel(bug.openedDate)}</time>
                            <small>截止 {dateLabel(bug.deadline)}</small>
                          </td>
                        </tr>
                      ))}
                  </tbody>
                </table>
              </div>
              <div className="bugs-pagination">
                <span>
                  共 {filtered.length} 个 BUG · 第 {currentPage + 1} / {pages}{" "}
                  页
                </span>
                <button
                  disabled={currentPage === 0}
                  onClick={() => setPage(currentPage - 1)}
                >
                  上一页
                </button>
                <button
                  disabled={currentPage >= pages - 1}
                  onClick={() => setPage(currentPage + 1)}
                >
                  下一页
                </button>
              </div>
            </>
          )}
        </>
      )}
      {edit && (
        <Modal
          title={action ? actionNames[action] : `BUG #${edit.remoteId}`}
          wide
          busy={busy}
          onClose={() => {
            setEdit(null);
            setAction(null);
          }}
        >
          {error && (
            <div className="error-panel" role="alert">
              {error}
            </div>
          )}
          {action ? (
            <form className="form-grid bugs-form" onSubmit={transition}>
              <p className="bugs-full">
                <strong>{edit.title}</strong>
              </p>
              {action === "resolve" && (
                <label>
                  解决方案
                  <GlassSelect
                    aria-label="解决方案"
                    value={resolution}
                    options={resolutionOptions}
                    disabled={busy}
                    onValueChange={setResolution}
                  />
                </label>
              )}
              {(action === "activate" ||
                (action === "resolve" && resolution === "fixed")) && (
                <label>
                  {action === "activate" ? "影响版本" : "解决版本"}
                  <input
                    value={build}
                    required={action === "resolve"}
                    disabled={busy}
                    onChange={(event) => setBuild(event.target.value)}
                  />
                  <small>
                    填写禅道版本编号，主干使用 trunk
                    {action === "activate"
                      ? "；多个版本用英文逗号分隔，留空保留原影响版本"
                      : ""}
                    。
                  </small>
                </label>
              )}
              {action === "resolve" && resolution === "duplicate" && (
                <label>
                  重复 BUG 编号
                  <input
                    type="number"
                    min="1"
                    step="1"
                    required
                    disabled={busy}
                    value={duplicate}
                    onChange={(event) => setDuplicate(event.target.value)}
                  />
                </label>
              )}
              {action !== "close" && (
                <label>
                  处理后指派给
                  <input
                    value={assignee}
                    required
                    disabled={busy}
                    onChange={(event) => setAssignee(event.target.value)}
                  />
                  <small>填写禅道账号；转派给其他人后将移出“我的 BUG”。</small>
                </label>
              )}
              <label className="bugs-full">
                操作备注
                <textarea
                  rows={4}
                  value={comment}
                  disabled={busy}
                  onChange={(event) => setComment(event.target.value)}
                />
              </label>
              <p className="bugs-full muted">
                确认后将直接更新禅道中的此 BUG。
              </p>
              <footer>
                <button
                  type="button"
                  disabled={busy}
                  onClick={() => {
                    setAction(null);
                    setError("");
                  }}
                >
                  返回详情
                </button>
                <button className="primary" disabled={busy}>
                  {busy ? "正在提交…" : `确认${actionNames[action]}`}
                </button>
              </footer>
            </form>
          ) : (
            <form
              className="form-grid bugs-form"
              onSubmit={(event) => {
                event.preventDefault();
                if (editing)
                  void run(
                    () =>
                      command<ZentaoBug>("zentao_bug_save", { value: edit }),
                    "BUG 已更新到禅道",
                    true,
                  );
              }}
            >
              <div className="bugs-full bugs-detail-bar">
                <span className={`bug-status bug-status-${edit.status}`}>
                  {statusNames[edit.status]}
                </span>
                <button
                  type="button"
                  disabled={busy}
                  onClick={() => void openRemote()}
                >
                  <ExternalLink size={15} />
                  在禅道打开
                </button>
              </div>
              <label className="bugs-full">
                标题
                <input
                  required
                  maxLength={255}
                  disabled={busy || !editing}
                  value={edit.title}
                  onChange={(event) =>
                    setEdit({ ...edit, title: event.target.value })
                  }
                />
              </label>
              <label>
                优先级
                <GlassSelect
                  aria-label="BUG 优先级"
                  options={levels.map((item) => ({
                    ...item,
                    label: `P${item.value}`,
                  }))}
                  value={String(edit.priority)}
                  disabled={busy || !editing}
                  onValueChange={(value) =>
                    setEdit({ ...edit, priority: Number(value) })
                  }
                />
              </label>
              <label>
                严重程度
                <GlassSelect
                  aria-label="BUG 严重程度"
                  options={levels}
                  value={String(edit.severity)}
                  disabled={busy || !editing}
                  onValueChange={(value) =>
                    setEdit({ ...edit, severity: Number(value) })
                  }
                />
              </label>
              <label className="bugs-full">
                指派给（禅道账号）
                <input
                  required
                  disabled={busy || !editing}
                  value={edit.assignedTo}
                  onChange={(event) =>
                    setEdit({ ...edit, assignedTo: event.target.value })
                  }
                />
              </label>
              {editing ? (
                <label className="bugs-full">
                  复现步骤
                  <textarea
                    rows={8}
                    value={edit.steps}
                    disabled={busy}
                    onChange={(event) =>
                      setEdit({ ...edit, steps: event.target.value })
                    }
                  />
                  <small>
                    支持纯文本和已有 HTML 格式；复杂排版、附件可在禅道中编辑。
                  </small>
                </label>
              ) : (
                <section className="bugs-full">
                  <h3>复现步骤</h3>
                  {edit.steps ? (
                    <div
                      className="bugs-steps"
                      dangerouslySetInnerHTML={{
                        __html: safeSteps(edit.steps),
                      }}
                    />
                  ) : (
                    <p className="muted">暂无复现步骤</p>
                  )}
                </section>
              )}
              <div className="bugs-full bugs-detail-meta">
                <span>
                  产品：{edit.productName || edit.productId || "未关联"}
                </span>
                <span>
                  项目：{edit.projectName || edit.projectId || "未关联"}
                </span>
                <span>创建：{dateLabel(edit.openedDate)}</span>
                <span>截止：{dateLabel(edit.deadline)}</span>
                {edit.resolution && (
                  <span>
                    解决方案：
                    {resolutionOptions.find(
                      (option) => option.value === edit.resolution,
                    )?.label || edit.resolution}
                  </span>
                )}
                {edit.resolvedDate && (
                  <span>解决：{dateLabel(edit.resolvedDate)}</span>
                )}
              </div>
              <footer className="bugs-detail-footer">
                {editing ? (
                  <>
                    <button
                      type="button"
                      disabled={busy}
                      onClick={() => {
                        const original = allBugs.find(
                          (bug) => bug.id === edit.id,
                        );
                        if (original) setEdit({ ...original });
                        setEditing(false);
                        setError("");
                      }}
                    >
                      取消编辑
                    </button>
                    <button
                      className="primary"
                      disabled={
                        busy || !edit.title.trim() || !edit.assignedTo.trim()
                      }
                    >
                      {busy ? "正在保存…" : "保存到禅道"}
                    </button>
                  </>
                ) : (
                  <>
                    <button
                      type="button"
                      disabled={busy}
                      onClick={() => setEditing(true)}
                    >
                      编辑 BUG
                    </button>
                    {edit.status === "active" && (
                      <button
                        type="button"
                        className="primary"
                        disabled={busy}
                        onClick={() => beginAction("resolve")}
                      >
                        <CheckCircle2 size={15} />
                        解决
                      </button>
                    )}
                    {edit.status === "resolved" && (
                      <button
                        type="button"
                        disabled={busy}
                        onClick={() => beginAction("close")}
                      >
                        <XCircle size={15} />
                        关闭
                      </button>
                    )}
                    {edit.status !== "active" && (
                      <button
                        type="button"
                        disabled={busy}
                        onClick={() => beginAction("activate")}
                      >
                        <RefreshCw size={15} />
                        激活
                      </button>
                    )}
                  </>
                )}
              </footer>
            </form>
          )}
        </Modal>
      )}
    </>
  );
}

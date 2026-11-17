import { assignmentApi, assignmentKey, useAssignmentGeneration } from "../lib/assignment-notifications";
import { QueryClientContext } from "@tanstack/react-query";
import { useContext, useEffect, useRef, useState, type FormEvent, type MouseEvent } from "react";
import {
  Bug,
  ArrowDown,
  ArrowUp,
  ArrowUpDown,
  CheckCircle2,
  BadgeCheck,
  ExternalLink,
  Plus,
  RefreshCw,
  Search,
  Settings2,
  XCircle,
  UserRoundCheck,
} from "lucide-react";
import { useNavigate, useSearchParams } from "react-router-dom";
import { DateTime } from "luxon";
import { openUrl } from "@tauri-apps/plugin-opener";
import GlassSelect from "../components/GlassSelect";
import ZentaoAssigneeSelect from "../components/ZentaoAssigneeSelect";
import Modal from "../components/Modal";
import BugSteps from "../components/BugSteps";
import BugHistory from "../components/BugHistory";
import BugComposer from "./BugComposer";
import { BugRichTextEditor } from "../components/TaskRichTextEditor";
import { command, native } from "../lib/api";
import type { PageProps, ZentaoBug } from "../lib/types";
import { bugGroupKey, buildBugProjectIndex } from "../lib/bug-projects";
import { sortBugs, type BugSortField, type BugSortDirection } from "../lib/bug-sort";
import "./bugs.css";

const statusNames = { active: "待解决", resolved: "已解决", closed: "已关闭" };
const matchesStatus = (bug: ZentaoBug, status: string) =>
  status === "all" || bug.status === status || (status === "mine-resolved" && bug.resolvedByMe === true);
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
type Action = "confirm" | "assign" | "resolve" | "close" | "activate";
const actionNames: Record<Action, string> = {
  confirm: "确认 BUG",
  assign: "指派 BUG",
  resolve: "解决 BUG",
  close: "关闭 BUG",
  activate: "激活 BUG",
};

function ConfirmationBadge({ confirmed }: { confirmed: ZentaoBug["confirmed"] }) {
  const state = confirmed === true ? "yes" : confirmed === false ? "no" : "unknown";
  return (
    <span className={`bug-confirmation bug-confirmation-${state}`}
      title={state === "unknown" ? "请同步 BUG 获取禅道中的确认状态" : undefined}>
      {state === "yes" ? "已确认" : state === "no" ? "未确认" : "待同步"}
    </span>
  );
}

function SeverityBadge({ severity }: { severity: number }) {
  const level = [1, 2, 3, 4].includes(severity) ? severity : undefined;
  const label = level ? `严重程度 ${level}` : "严重程度未知";
  return (
    <span className="bug-severity" data-severity={level} role="img" aria-label={label} title={label}>
      {/* 与禅道 22.0 ZUI 的 severity 图形保持一致，数字与轮廓共用等级颜色。 */}
      <svg viewBox="0 0 27 30" aria-hidden="true" focusable="false">
        <path fill="currentColor" d="M2.267 19.913c-.056 4.557 4.911 8.235 11.308 8.022 6.095-.203 11.157-4.713 11.159-9.517.001-2.764-.97-5.625-2.71-8.106a4.65 4.65 0 0 1-1.263 1.788l-1.633 1.409c-.272.233-.707.106-.793-.233l-.513-2.027c-.793-3.13-2.841-6.29-5.016-7.984-.676 4.138-2.935 8.507-6.138 11.387l-1.394 1.254c-.247.222-.652.138-.779-.162l-.578-1.364c-1.19 2.264-1.628 3.711-1.65 5.533M19.706 5.856c-.193-.446.375-.837.76-.523 3.993 3.26 6.536 8.154 6.534 13-.002 5.781-5.822 11.407-13.346 11.657C5.877 30.249-.069 25.543.001 19.804c.027-2.297.621-4.064 1.921-6.527.018-.035.638-1.192.802-1.512.28-.544.482-.985.639-1.405.218-.583.342-1.112.372-1.63.025-.43.602-.594.87-.247.647.838.992 1.915 1.159 3.26q0 .166.045.614l.18-.176q.112-.113.174-.182c-.015.026-.018.035-.02.046.025-.042.063-.124.124-.191 2.986-3.33 4.577-8.139 4.473-11.38-.01-.334.344-.566.667-.438 2.827 1.118 5.034 3.318 6.7 6.187a21 21 0 0 1 1.614 3.484c.408-.926.594-2.441-.015-3.85" />
        <text x="13.5" y="26" textAnchor="middle" fill="currentColor">{level ?? "?"}</text>
      </svg>
    </span>
  );
}

function dateLabel(value?: string) {
  if (!value || value.startsWith("0000")) return "未设置";
  const date = DateTime.fromISO(value.replace(" ", "T"));
  return date.isValid
    ? date.toFormat(value.length === 10 ? "yyyy-MM-dd" : "yyyy-MM-dd HH:mm")
    : "未设置";
}

export default function Bugs({
  data,
  refresh,
  notify,
  search = "",
  workspaceId = "",
}: PageProps & { search?: string; workspaceId?: string }) {
  const navigate = useNavigate();
  const [params, setParams] = useSearchParams();
  const filter = ["all", "active", "resolved", "closed", "mine-resolved"].includes(
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
  const [confirmation, setConfirmation] = useState("all");
  const [query, setQuery] = useState("");
  const [page, setPage] = useState(0);
  const [sortField, setSortField] = useState<BugSortField>("openedDate");
  const [sortDirection, setSortDirection] = useState<BugSortDirection>("desc");
  const [edit, setEdit] = useState<ZentaoBug | null>(null);
  const [createOpen, setCreateOpen] = useState(false);
  const [editing, setEditing] = useState(false);
  const notificationClient = useContext(QueryClientContext);
  const notificationGeneration = useAssignmentGeneration(workspaceId);
  const readNotificationTarget = useRef("");
  const requestedBugId = params.get("bugId");
  const notificationTarget = `${workspaceId}:${requestedBugId}:${params.get("notificationId") || ""}`;
  const openedNotificationId = useRef<string | null>(null);
  useEffect(() => {
    if (!requestedBugId) { openedNotificationId.current = null; return; }
    if (openedNotificationId.current === notificationTarget) return;
    const bug = data.bugs?.find(value => value.id === requestedBugId);
    if (bug) { openedNotificationId.current = notificationTarget; setEdit({...bug}); setEditing(false); setAction(null); setError(""); }
  }, [requestedBugId, notificationTarget, data.bugs]);
  useEffect(() => {
    if (!edit?.id || edit.source === "local") { readNotificationTarget.current = ""; return; }
    if (!native || !workspaceId || !notificationGeneration) return;
    const target = `${workspaceId}:${edit.id}`;
    if (readNotificationTarget.current === target) return;
    readNotificationTarget.current = target;
    void assignmentApi.readEntity(workspaceId, "bug", edit.id, notificationGeneration)
      .then(() => notificationClient?.invalidateQueries({queryKey: assignmentKey(workspaceId)}))
      .catch(error => notify(`无法清除 BUG 未读提醒：${error instanceof Error ? error.message : String(error)}`));
  }, [workspaceId, edit?.id, edit?.source, notificationClient, notificationGeneration]);

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
  const createProjects = data.projects.filter(project =>
    project.source === "zentao" && project.remoteId &&
    data.connections.some(item => item.id === project.connectionId && item.enabled && item.hasCredential !== false),
  );
  const allBugs = data.bugs ?? [];
  const projectFilter = params.get("project") || "all";
  const projectGroups = buildBugProjectIndex(data.projects, allBugs);
  const projectBugs =
    projectFilter === "all"
      ? allBugs
      : allBugs.filter(
          (bug) =>
            bugGroupKey(bug) === projectFilter,
        );
  const words = `${search} ${query}`
    .trim()
    .toLocaleLowerCase()
    .split(/\s+/)
    .filter(Boolean);
  const filtered = projectBugs.filter(
    (bug) =>
      matchesStatus(bug, filter) &&
      (severity === "all" || bug.severity === Number(severity)) &&
      (confirmation === "all" ||
        (confirmation === "confirmed" && bug.confirmed === true) ||
        (confirmation === "unconfirmed" && bug.confirmed === false)) &&
      words.every((word) =>
        `${bug.source === "local" ? "本地 BUG" : bug.remoteId} ${bug.title} ${bug.productName ?? ""} ${bug.projectName ?? ""} ${bug.assignedToName ?? ""} ${bug.assignedTo}`
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
    notificationTarget?: "statusbar",
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
      notify(typeof result === "string" ? result : message, notificationTarget);
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
  function onComposerSaved(message: string, status?: ZentaoBug["status"]) {
    setCreateOpen(false);
    setEdit(null);
    if (status) {
      const next = new URLSearchParams(params);
      next.set("status", status);
      next.delete("project");
      next.delete("bugId");
      next.delete("notificationId");
      setParams(next);
      setPage(0);
      setQuery("");
      setSeverity("all");
      setConfirmation("all");
    }
    notify(message);
    void refresh().catch(() => setError("BUG 已保存，但列表刷新失败，请刷新页面查看。"));
  }
  function beginAction(next: Action, event: MouseEvent<HTMLButtonElement>) {
    // 阻止同一次点击在切换表单后触发新确认按钮的默认提交。
    event.preventDefault();
    if (edit?.readOnly) return;
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
    if (!edit || edit.readOnly || !action) return;
    const payload: Record<string, unknown> = { comment };
    if (action !== "confirm" && action !== "close") payload.assignedTo = assignee.trim();
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
          <p>记录本地 BUG，或提交并处理禅道 BUG</p>
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
            disabled={busy || !connection?.enabled}
            onClick={() =>
              void run(
                () =>
                  command<string>("zentao_bugs_sync", {
                    connectionId: connection!.id,
                  }),
                "BUG 同步完成",
                false,
                "statusbar",
              )
            }
          >
            <RefreshCw size={16} />
            {busy ? "正在处理…" : "同步我的 BUG"}
          </button>
          <button type="button" className="primary" disabled={busy} onClick={() => { setError(""); setCreateOpen(true); }}>
            <Plus size={16} /> 新建 BUG
          </button>
        </div>
      </div>
      {!edit && error && (
        <div className="error-panel" role="alert">
          {error}
        </div>
      )}
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
                ...(connection ? [{ value: "mine-resolved", label: "我解决的" }] : []),
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
                    {projectBugs.filter(bug => matchesStatus(bug, item.value)).length}
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
              <GlassSelect
                aria-label="是否确认"
                value={confirmation}
                onValueChange={(value) => {
                  setConfirmation(value);
                  setPage(0);
                }}
                options={[
                  { value: "all", label: "全部确认状态" },
                  { value: "confirmed", label: "已确认" },
                  { value: "unconfirmed", label: "未确认" },
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
          {connection && !connection.enabled && (
            <p className="error" role="status">
              禅道连接已停用，请在禅道设置中启用后同步。
            </p>
          )}
          <p className="bugs-scope">
            {connection?.lastBugSync
              ? `最近同步：${new Date(connection.lastBugSync).toLocaleString("zh-CN")}。`
              : connection ? "尚未同步禅道 BUG。" : "尚未配置禅道连接。"}
            {connection
              ? "列表显示本地 BUG、禅道中指派给我的 BUG，以及禅道中由我解决的记录。"
              : "可直接创建本地 BUG。"}
          </p>
          {filtered.length === 0 ? (
            <div className="empty">
              <Bug size={32} />
              <h3>
                {allBugs.length ? "没有符合条件的 BUG" : "暂无 BUG"}
              </h3>
              <p>
                {allBugs.length
                  ? "试试切换项目、状态、严重程度、是否确认或调整关键词。"
                  : connection?.enabled ? "可以新建 BUG，或同步禅道中的最新指派。" : "点击“新建 BUG”记录本地问题。"}
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
                      <th scope="col" aria-sort={columnOrder(["priority"])}>{sortButton("priority", "优先级")}</th>
                      <th scope="col" className="bugs-severity-cell" aria-sort={columnOrder(["severity"])}>{sortButton("severity", "严重程度")}</th>
                      <th scope="col">产品 / 项目</th>
                      <th scope="col" aria-sort={columnOrder(["confirmed"])}>{sortButton("confirmed", "是否确认")}</th>
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
                              <span>{bug.source === "local" ? "本地 BUG" : `#${bug.remoteId}${bug.readOnly ? " · 我解决的" : ""}`}</span>
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
                          </td>
                          <td className="bugs-severity-cell"><SeverityBadge severity={bug.severity} /></td>
                          <td>
                            {bug.source === "local" ? "—" : <>{bug.productName ||
                              (bug.productId
                                ? `产品 #${bug.productId}`
                                : "未关联产品")}
                            <small>
                              {bug.projectName ||
                                (bug.projectId
                                  ? `项目 #${bug.projectId}`
                                  : "未关联项目")}
                            </small>
                            </>}
                          </td>
                          <td>{bug.source === "local" ? "—" : <ConfirmationBadge confirmed={bug.confirmed} />}</td>
                          <td>
                            {bug.source === "local" ? "—" : bug.assignedToName || bug.assignedTo || "未指派"}
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
      {(createOpen || edit?.source === "local") && (
        <BugComposer key={createOpen ? "create" : edit?.id}
          workspaceId={workspaceId} projects={createProjects}
          bug={createOpen ? undefined : edit ?? undefined}
          onClose={() => { setCreateOpen(false); setEdit(null); }}
          onSaved={onComposerSaved} />
      )}
      {edit && edit.source !== "local" && (
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
            <form key="action" className="form-grid bugs-form" onSubmit={transition}>
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
              {action !== "close" && action !== "confirm" && (
                <label>
                  {action === "assign" ? "指派给" : "处理后指派给"}
                  <ZentaoAssigneeSelect key={edit.id} bugId={edit.id} value={assignee} currentName={edit.assignedToName}
                    label={action === "assign" ? "指派给" : "处理后指派给"} disabled={busy} onChange={account => setAssignee(account)} />
                  <small>可按姓名或账号搜索。转派后，只有由你解决的 BUG 仍会显示。</small>
                </label>
              )}
              <label className="bugs-full">
                操作备注
                <textarea
                  aria-label="操作备注"
                  rows={4}
                  value={comment}
                  disabled={busy}
                  onChange={(event) => setComment(event.target.value)}
                />
              </label>
              <p className="bugs-full muted">
                {action === "confirm" ? "提交后将调用禅道的确认功能，将此 BUG 标记为已确认。" : "确认后将直接更新禅道中的此 BUG。"}
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
                <button type="submit" className="primary" disabled={busy || (action === "assign" && !assignee.trim())}>
                  {busy ? "正在提交…" : action === "confirm" ? "确认 BUG" : action === "assign" ? "提交指派" : `确认${actionNames[action]}`}
                </button>
              </footer>
            </form>
          ) : (
            <form
              key="detail"
              className="form-grid bugs-form"
              onSubmit={(event) => {
                event.preventDefault();
                if (editing && !edit.readOnly)
                  void run(
                    () =>
                      command<ZentaoBug>("zentao_bug_save", { value: edit }),
                    "BUG 已更新到禅道",
                    true,
                  );
              }}
            >
              <div className="bugs-full bugs-detail-bar">
                <div className="bugs-detail-badges">
                  <span className={`bug-status bug-status-${edit.status}`}>
                    {statusNames[edit.status]}
                  </span>
                  <ConfirmationBadge confirmed={edit.confirmed} />
                  {edit.readOnly && <span className="muted">我解决的 · 只读</span>}
                </div>
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
                指派给
                <ZentaoAssigneeSelect key={edit.id} bugId={edit.id} value={edit.assignedTo} currentName={edit.assignedToName}
                  active={editing} disabled={busy} onChange={(account, realname) =>
                    setEdit(previous => previous ? { ...previous, assignedTo: account, assignedToName: realname } : previous)
                  } />
              </label>
              {editing ? (
                <section className="bugs-full bugs-step-editor" aria-labelledby="bug-steps-title">
                  <h3 id="bug-steps-title">复现步骤</h3>
                  <BugRichTextEditor
                    key={`${edit.id}:${edit.revision}`}
                    bugId={edit.id}
                    value={edit.steps}
                    disabled={busy}
                    onChange={(steps) =>
                      setEdit(previous => previous ? { ...previous, steps } : previous)
                    }
                  />
                </section>
              ) : (
                <section className="bugs-full">
                  <h3>复现步骤</h3>
                  {edit.steps ? (
                    <BugSteps key={`${edit.id}:${edit.revision}`} bugId={edit.id} value={edit.steps} />
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
              {!editing && <BugHistory key={`${edit.id}:${edit.revision}`} bugId={edit.id} revision={edit.revision} />}
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
                ) : !edit.readOnly ? (
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
                        disabled={busy || edit.confirmed !== false}
                        title={edit.confirmed === true ? "此 BUG 已确认" : edit.confirmed !== false ? "请先同步 BUG 获取确认状态" : undefined}
                        onClick={(event) => beginAction("confirm", event)}
                      >
                        <BadgeCheck size={15} />
                        确认
                      </button>
                    )}
                    {edit.status !== "closed" && (
                      <button
                        type="button"
                        disabled={busy}
                        onClick={(event) => beginAction("assign", event)}
                      >
                        <UserRoundCheck size={15} />
                        指派
                      </button>
                    )}
                    {edit.status === "active" && (
                      <button
                        type="button"
                        className="primary"
                        disabled={busy}
                        onClick={(event) => beginAction("resolve", event)}
                      >
                        <CheckCircle2 size={15} />
                        解决
                      </button>
                    )}
                    {edit.status === "resolved" && (
                      <button
                        type="button"
                        disabled={busy}
                        onClick={(event) => beginAction("close", event)}
                      >
                        <XCircle size={15} />
                        关闭
                      </button>
                    )}
                    {edit.status !== "active" && (
                      <button
                        type="button"
                        disabled={busy}
                        onClick={(event) => beginAction("activate", event)}
                      >
                        <RefreshCw size={15} />
                        激活
                      </button>
                    )}
                  </>
                ) : null}
              </footer>
            </form>
          )}
        </Modal>
      )}
    </>
  );
}

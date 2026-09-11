import { Fragment, useId, useRef, useState } from "react";
import DOMPurify from "dompurify";
import TaskRichTextEditor from "../components/TaskRichTextEditor";
import { taskNotesText } from "../lib/task-rich-text";
import "./projects.css";
import { buildBugProjectIndex, projectBugKey } from "../lib/bug-projects";
import { useNavigate, useSearchParams } from "react-router-dom";
import {
  Plus,
  Columns3,
  List,
  CalendarDays,
  CircleAlert,
  CircleDashed,
  CircleDot,
  CircleCheck,
  CircleX,
  ArrowUpRight,
  FolderKanban,
  Pencil,
  Trash2,
  GripVertical,
  Undo2,
} from "lucide-react";
import {
  DndContext,
  DragOverlay,
  useDraggable,
  useDroppable,
  closestCorners,
  pointerWithin,
  type CollisionDetection,
  KeyboardSensor,
  PointerSensor,
  useSensor,
  useSensors,
  type DragEndEvent,
} from "@dnd-kit/core";
import CalendarView from "./Calendar";
import { DateTime } from "luxon";
import Modal from "../components/Modal";
import GlassSelect from "../components/GlassSelect";
import { zentaoDetailUrl } from "../lib/zentao-links";
import { openUrl } from "@tauri-apps/plugin-opener";
import { api } from "../lib/api";
import { createSchedule, taskScheduleDates } from "../lib/schedule";
import { isTaskExpired } from "../lib/task-expiry";
import { useExpiryRefresh } from "../lib/use-expiry-refresh";
import { buildBugStatistics } from "../lib/bug-statistics";
import {
  statuses,
  type Task,
  type Project,
  type PageProps,
  type Status,
  type Application,
} from "../lib/types";

function OverviewApp({
  app,
  category,
  notify,
}: {
  app: Application;
  category: string;
  notify: PageProps["notify"];
}) {
  const [failedLogo, setFailedLogo] = useState<string>();
  return (
    <button
      className="app-card overview-app"
      title={[app.name, app.description, app.url].filter(Boolean).join("\n")}
      onClick={async () => {
        try {
          const { openUrl } = await import("@tauri-apps/plugin-opener");
          await openUrl(app.url);
        } catch (error) {
          notify(String(error));
        }
      }}
    >
      <span className="app-logo" aria-hidden="true">
        {app.logo && failedLogo !== app.logo ? (
          <img src={app.logo} alt="" onError={() => setFailedLogo(app.logo)} />
        ) : (
          app.name.slice(0, 1)
        )}
      </span>
      <span className="overview-app-content">
        <span className="overview-app-heading">
          <strong>{app.name}</strong>
          <span className="tag overview-app-category" title={category}>
            {category}
          </span>
        </span>
        {app.description?.trim() && (
          <span className="overview-app-description">{app.description}</span>
        )}
        <small>{new URL(app.url).hostname}</small>
      </span>
      <ArrowUpRight
        size={14}
        className="overview-app-arrow"
        aria-hidden="true"
      />
    </button>
  );
}

const statusIcons = {
  todo: CircleDashed,
  doing: CircleDot,
  done: CircleCheck,
  closed: CircleX,
};

// 空列可能和最长任务列一样高；按四角距离会误选相邻卡片。
// 鼠标拖动优先使用实际落点，键盘拖动保留几何导航。
const boardCollision: CollisionDetection = (args) => {
  const slots = args.droppableContainers.filter(
    (item) => item.data.current?.placement,
  );
  if (!args.pointerCoordinates)
    return closestCorners({ ...args, droppableContainers: slots });
  const hits = pointerWithin(args);
  const exact = hits.filter((hit) => String(hit.id).startsWith("slot:"));
  if (exact.length) return exact;
  const column = hits.find((hit) => String(hit.id) in statuses);
  if (!column) return [];
  return closestCorners({
    ...args,
    collisionRect: {
      ...args.collisionRect,
      top: args.pointerCoordinates.y,
      bottom: args.pointerCoordinates.y,
      left: args.pointerCoordinates.x,
      right: args.pointerCoordinates.x,
      width: 0,
      height: 0,
    },
    droppableContainers: slots.filter(
      (slot) => slot.data.current?.placement.status === column.id,
    ),
  });
};
function BoardSlot({
  status,
  beforeId,
  active,
}: {
  status: Status;
  beforeId: string | null;
  active: boolean;
}) {
  const { setNodeRef, isOver } = useDroppable({
    id: `slot:${status}:${beforeId ?? "end"}`,
    data: { placement: { status, beforeId } },
    disabled: !active,
  });
  return (
    <div
      ref={setNodeRef}
      className={`board-slot${active ? " dragging" : ""}${isOver ? " insertion" : ""}`}
    >
      {isOver && <span>放到这里</span>}
    </div>
  );
}
const zone = Intl.DateTimeFormat().resolvedOptions().timeZone;
const dateLabel = (date?: string) =>
  date ? DateTime.fromISO(date).toFormat("M月d日") : "未排期";
function scheduleLabel(t: Task) {
  if (!t.schedule) return "未排期";
  if (t.schedule.kind === "all_day")
    return dateLabel(t.schedule.start) + " 全天";
  return (
    DateTime.fromISO(t.schedule.start)
      .setZone(t.schedule.timezone)
      .toFormat("M月d日 HH:mm") +
    (t.schedule.end
      ? " – " +
        DateTime.fromISO(t.schedule.end)
          .setZone(t.schedule.timezone)
          .toFormat(
            DateTime.fromISO(t.schedule.end)
              .setZone(t.schedule.timezone)
              .hasSame(
                DateTime.fromISO(t.schedule.start).setZone(t.schedule.timezone),
                "day",
              )
              ? "HH:mm"
              : "M月d日 HH:mm",
          )
      : "")
  );
}
const blankTask = (): Task => ({
  id: crypto.randomUUID(),
  title: "",
  notes: "",
  source: "local",
  status: "todo",
  priority: "normal",
  sortOrder: Date.now(),
});

function RequiredMark() {
  return <span className="required-mark" aria-hidden="true">*</span>;
}

export function TaskEditor({
  task,
  data,
  onClose,
  refresh,
  notify,
}: { task: Task; onClose: () => void } & PageProps) {
  useExpiryRefresh();
  const initialExecution =
    !task.revision &&
    data.projects.some(
      (project) => project.id === task.projectId && project.source === "zentao",
    );
  const [value, setValue] = useState<Task>(
      initialExecution ? { ...task, status: "todo" } : task,
    ),
    [error, setError] = useState(""),
    [busy, setBusy] = useState(false);
  const saving = useRef(false);
  const [mediaBusy, setMediaBusy] = useState(false);
  const [saved, setSaved] = useState(false);
  const [projectHelp, setProjectHelp] = useState(false);
  const projectHelpId = useId();
  const selectedProject = data.projects.find(
    (project) => project.id === value.projectId,
  );
  const creatingExecution =
    !task.revision && selectedProject?.source === "zentao";
  const zentaoSchedule = value.source === "zentao" || creatingExecution;
  const [allDay, setAllDay] = useState(
    task.source === "zentao" ||
      initialExecution ||
      task.schedule?.kind === "all_day",
  );
  const local = (v?: string) =>
    v
      ? DateTime.fromISO(v)
          .setZone(task.schedule?.timezone || zone)
          .toFormat("yyyy-MM-dd'T'HH:mm")
      : "";
  const [start, setStart] = useState(
    task.schedule?.kind === "all_day"
      ? task.schedule.start
      : task.source === "zentao" || initialExecution
        ? local(task.schedule?.start).slice(0, 10) ||
          (initialExecution
            ? DateTime.now()
                .setZone(task.schedule?.timezone || zone)
                .toISODate() || ""
            : "")
        : local(task.schedule?.start),
  );
  const [end, setEnd] = useState(
    task.schedule?.kind === "all_day"
      ? task.schedule.end
        ? DateTime.fromISO(task.schedule.end).minus({ days: 1 }).toISODate() ||
          ""
        : ""
      : task.source === "zentao" || initialExecution
        ? local(task.schedule?.end).slice(0, 10)
        : local(task.schedule?.end),
  );
  const [timezone, setTimezone] = useState(task.schedule?.timezone || zone);
  const requiredExecutionDates = Boolean(zentaoSchedule && (creatingExecution || task.schedule || start || end));
  const requiredStart = requiredExecutionDates || Boolean(end);
  async function submit(e: React.FormEvent) {
    e.preventDefault();
    if (saving.current || saved || mediaBusy) return;
    saving.current = true;
    setError("");
    setBusy(true);
    try {
      if (creatingExecution && /data:image\//i.test(value.notes || ""))
        throw Error("请先移除本地图片并保存禅道执行，再通过插入图片上传到禅道");
      if (creatingExecution && (!start || !end))
        throw Error("创建禅道执行需要填写计划开始和结束日期");
      const schedule = createSchedule(start, end, timezone, allDay);
      await api.save("task", { ...value, schedule });
      setSaved(true);
      try {
        await refresh();
      } catch {
        setError(
          "保存已成功，但页面刷新失败。请关闭弹窗后刷新页面核对，不要重复创建。",
        );
        return;
      }
      onClose();
      notify(
        creatingExecution
          ? "已在禅道创建执行并关联任务"
          : value.source === "zentao"
            ? "任务已保存并同步到禅道"
            : "任务已保存",
      );
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
      saving.current = false;
    }
  }
  return (
    <Modal
      title={task.revision ? "任务详情" : "新建任务"}
      onClose={onClose}
      busy={busy || mediaBusy}
    >
      <form onSubmit={submit} className="form-grid">
        {isTaskExpired(value) && <span className="tag expired">已过期</span>}
        <label>
          <span>任务名称<RequiredMark /></span>
          <input
            required
            maxLength={160}
            disabled={busy || saved}
            value={value.title}
            onChange={(e) => setValue({ ...value, title: e.target.value })}
          />
        </label>
        <div className="task-project-field">
          <div className="task-project-label">
            <span>所属项目{zentaoSchedule && <RequiredMark />}</span>
            {creatingExecution && (
              <button
                type="button"
                className="icon-button"
                aria-label="禅道执行创建说明"
                title="禅道执行创建说明"
                aria-expanded={projectHelp}
                aria-controls={projectHelpId}
                onClick={() => setProjectHelp(!projectHelp)}
              >
                <CircleAlert size={15} />
              </button>
            )}
          </div>
          <GlassSelect
            aria-label="所属项目"
            aria-required={zentaoSchedule}
            value={value.projectId || ""}
            disabled={value.source === "zentao" || busy || saved}
            onValueChange={(projectId) => {
              setProjectHelp(false);
              const remote =
                !task.revision &&
                data.projects.some(
                  (project) =>
                    project.id === projectId && project.source === "zentao",
                );
              setValue({
                ...value,
                projectId: projectId || undefined,
                status: remote ? "todo" : value.status,
              });
              if (remote) {
                setAllDay(true);
                setStart(
                  start
                    ? start.slice(0, 10)
                    : DateTime.now().setZone(timezone).toISODate() || "",
                );
                setEnd(end ? end.slice(0, 10) : "");
              }
            }}
            options={[
              { value: "", label: "未关联项目" },
              ...data.projects.map((p) => ({
                value: p.id,
                label: p.name + (p.source === "zentao" ? " · 禅道项目" : ""),
              })),
            ]}
          />
        </div>
        {creatingExecution && projectHelp && (
          <p id={projectHelpId} className="task-project-help">
            保存后将在“{selectedProject.name}
            ”创建禅道执行。默认迭代、短期，由当前禅道账号负责；任务名称和备注对应执行名称和描述。计划结束日期包含当天，新执行为待做，创建后可变更状态。
          </p>
        )}
        <div className="two-fields">
          <label>
            <span>任务状态<RequiredMark /></span>
            <GlassSelect
              aria-label="任务状态"
              required
              value={value.status}
              disabled={creatingExecution || busy || saved}
              onValueChange={(status) =>
                setValue({ ...value, status: status as Status })
              }
              options={Object.entries(statuses).map(([value, label]) => ({
                value,
                label,
              }))}
            />
          </label>
          <label>
            <span>优先级<RequiredMark /></span>
            <GlassSelect
              aria-label="优先级"
              required
              disabled={busy || saved}
              value={value.priority}
              onValueChange={(priority) =>
                setValue({
                  ...value,
                  priority: priority as Task["priority"],
                })
              }
              options={[
                { value: "low", label: "低" },
                { value: "normal", label: "普通" },
                { value: "high", label: "高" },
              ]}
            />
          </label>
        </div>
        <label className="check-label">
          <input
            type="checkbox"
            checked={allDay}
            disabled={zentaoSchedule || busy || saved}
            onChange={(e) => {
              setAllDay(e.target.checked);
              setStart("");
              setEnd("");
            }}
          />
          全天安排
        </label>
        <div className="two-fields">
          <label>
            <span>开始{allDay ? "日期" : "时间"}{requiredStart && <RequiredMark />}</span>
            <input
              type={allDay ? "date" : "datetime-local"}
              required={requiredStart}
              disabled={busy || saved}
              value={start}
              onChange={(e) => setStart(e.target.value)}
            />
          </label>
          <label>
            <span>结束{allDay ? "日期" : "时间"}{requiredExecutionDates && <RequiredMark />}</span>
            <input
              type={allDay ? "date" : "datetime-local"}
              required={requiredExecutionDates}
              disabled={busy || saved}
              min={allDay && start ? start : undefined}
              value={end}
              onChange={(e) => setEnd(e.target.value)}
            />
          </label>
        </div>
        <div className="two-fields">
          <label>
            <span>时区<RequiredMark /></span>
            <input
              required
              value={timezone}
              disabled={busy || saved}
              onChange={(e) => setTimezone(e.target.value)}
            />
          </label>
        </div>
        {value.source === "zentao" && (
          <p className="notice">
            禅道 {value.remoteType} #{value.remoteId} · {value.remoteStatus}
            。保存名称、状态、排期及备注时会同步更新禅道执行；所属项目保持一致。禅道排期精确到日期，修改排期需同时填写开始和结束日期。
          </p>
        )}
        <div className="task-notes-field">
          <span>备注</span>
          <TaskRichTextEditor key={value.id} task={value} value={value.notes || ""}
            disabled={busy || saved} remoteDraft={creatingExecution}
            onChange={(notes) => setValue(previous => ({ ...previous, notes }))}
            onBusyChange={setMediaBusy} />
        </div>
        {error && (
          <p role="alert" className="error">
            {error}
          </p>
        )}
        <footer>
          {task.revision && task.source === "local" && (
            <button
              type="button"
              className="danger"
              disabled={busy}
              onClick={async () => {
                if (!confirm("删除这个任务？")) return;
                setBusy(true);
                try {
                  await api.remove("task", task.id, task.revision);
                  await refresh();
                  onClose();
                  notify("任务已删除");
                } catch (e) {
                  setError((e as Error).message);
                } finally {
                  setBusy(false);
                }
              }}
            >
              <Trash2 size={15} />
              删除任务
            </button>
          )}
          <button type="button" onClick={onClose} disabled={busy || mediaBusy}>
            {saved ? "关闭" : "取消"}
          </button>
          <button className="primary" disabled={busy || saved || mediaBusy}>
            {saved
              ? "已保存"
              : busy
                ? "保存中…"
                : creatingExecution
                  ? "创建禅道执行"
                  : "保存"}
          </button>
        </footer>
      </form>
    </Modal>
  );
}
function Card({
  task,
  onOpen,
  project,
  onRemote,
  disabled = false,
}: {
  task: Task;
  onOpen: () => void;
  project?: string;
  onRemote?: () => void;
  disabled?: boolean;
}) {
  const dates = taskScheduleDates(task.schedule);
  const { attributes, listeners, setNodeRef, isDragging } = useDraggable({
    id: task.id,
    disabled,
  });
  return (
    <article
      ref={setNodeRef}
      className={`task-card${task.status === "closed" ? " task-is-closed" : ""}`}
      style={{ opacity: isDragging ? 0.3 : undefined }}
    >
      <div className="card-meta">
        <span className={task.source === "zentao" ? "source-remote" : ""}>
          {task.source === "zentao"
            ? `禅道${task.remoteType === "task" ? "任务" : "执行"} #${task.remoteId}`
            : "个人任务"}
        </span>
        <div className="task-card-actions">
          {onRemote && (
            <button
              className="icon-button"
              title="在禅道中打开详情"
              aria-label={`在禅道中打开：${task.title}`}
              onClick={onRemote}
            >
              <ArrowUpRight size={14} />
            </button>
          )}
          <button
            disabled={disabled}
            className="icon-button grip"
            {...listeners}
            {...attributes}
            title="拖动任务"
          >
            <GripVertical size={15} />
          </button>
        </div>
      </div>
      <button className="task-title" onClick={onOpen}>
        {task.title}
      </button>
      <p>{taskNotesText(task.notes || "")}</p>
      {task.source === "zentao" && task.remoteExecutionOwner && (
        <small className="muted">执行负责人：{task.remoteExecutionOwner}</small>
      )}
      <div className="tags">
        {isTaskExpired(task) && <span className="tag expired">已过期</span>}
        {task.priority === "high" && (
          <span className="tag warning">高优先级</span>
        )}
        {project && <span className="tag">{project}</span>}
      </div>
      <div className="card-bottom task-card-dates">
        <CalendarDays size={13} aria-hidden="true" />
        <div>
          <span>
            <span>开始</span>
            <span>{dates.start}</span>
          </span>
          <span>
            <span>结束</span>
            <span>{dates.end}</span>
          </span>
        </div>
        {task.schedule?.kind === "all_day" && <small>全天</small>}
      </div>
    </article>
  );
}
function Column({
  status,
  children,
  count,
}: {
  status: Status;
  children: React.ReactNode;
  count: number;
}) {
  const { setNodeRef, isOver } = useDroppable({ id: status });
  const Icon = statusIcons[status];
  return (
    <section
      ref={setNodeRef}
      className={"board-column " + (isOver ? "drop-over" : "")}
    >
      <h3 className={status}>
        <Icon size={16} />
        {statuses[status]}
        <span>{count}</span>
      </h3>
      {children}
    </section>
  );
}
export default function Work({
  data,
  refresh,
  notify,
  page = "tasks",
  search = "",
  onNew,
}: { page?: string; search?: string; onNew?: () => void } & PageProps) {
  useExpiryRefresh();
  const navigate = useNavigate();
  const bugStats = buildBugStatistics(data.bugs);
  const [params] = useSearchParams();
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 5 } }),
    useSensor(KeyboardSensor),
  );
  const [undo, setUndo] = useState<{
    before: Task;
    beforeId: string | null;
    revision: number;
  } | null>(null);
  const [dragging, setDragging] = useState<Task | null>(null);
  const moveLock = useRef(false);
  const [moving, setMoving] = useState(false);
  const [moveError, setMoveError] = useState("");
  const [view, setView] = useState("board"),
    [project, setProject] = useState(params.get("project") || "all"),
    [status, setStatus] = useState("all"),
    [editing, setEditing] = useState<Task | null>(null);
  const list = data.tasks
    .filter(
      (t) =>
        (project === "all" || t.projectId === project) &&
        (status === "all" || t.status === status) &&
        `${t.title} ${t.notes}`.toLowerCase().includes(search.toLowerCase()),
    )
    .sort(
      (a, b) =>
        a.sortOrder - b.sortOrder || (a.id < b.id ? -1 : a.id > b.id ? 1 : 0),
    );
  const today = DateTime.local().toISODate();
  const todayTasks = list.filter(
    (t) =>
      t.schedule &&
      (t.schedule.kind === "all_day"
        ? t.schedule.start <= today! &&
          (t.schedule.end ||
            DateTime.fromISO(t.schedule.start).plus({ days: 1 }).toISODate() ||
            "") > today!
        : DateTime.fromISO(t.schedule.start) <
            DateTime.local().plus({ days: 1 }).startOf("day") &&
          DateTime.fromISO(t.schedule.end || t.schedule.start) >=
            DateTime.local().startOf("day")),
  );
  async function move(event: DragEndEvent) {
    setDragging(null);
    const t = data.tasks.find((task) => task.id === event.active.id);
    const placement = event.over?.data.current?.placement as
      { status: Status; beforeId: string | null } | undefined;
    if (moveLock.current || !t || !placement || placement.beforeId === t.id)
      return;
    const original = data.tasks
      .filter((task) => task.status === t.status)
      .sort(
        (a, b) =>
          a.sortOrder - b.sortOrder || (a.id < b.id ? -1 : a.id > b.id ? 1 : 0),
      );
    const beforeId =
      original[original.findIndex((task) => task.id === t.id) + 1]?.id ?? null;
    if (placement.status === t.status && placement.beforeId === beforeId)
      return;
    moveLock.current = true;
    setMoving(true);
    setMoveError("");
    try {
      const saved = await api.save("task", {
        ...t,
        status: placement.status,
        boardBeforeId: placement.beforeId,
      });
      setUndo({
        before: { ...saved, status: t.status },
        beforeId,
        revision: saved.revision!,
      });
      try {
        await refresh();
      } catch {
        setMoveError("任务已保存，但页面刷新失败。请刷新核对，不要重复拖动。");
        return;
      }
      notify(
        t.source === "zentao" && t.status !== placement.status
          ? "状态已同步到禅道，任务位置已保存"
          : "任务位置已保存",
      );
    } catch (e) {
      setMoveError((e as Error).message);
      notify((e as Error).message);
    } finally {
      moveLock.current = false;
      setMoving(false);
    }
  }
  async function openRemote(entity: Task | Project) {
    try {
      await openUrl(zentaoDetailUrl(entity, data.connections));
    } catch (error) {
      notify(error instanceof Error ? error.message : String(error));
    }
  }
  const taskRows = (items: Task[]) => (
    <div className="table-wrap">
      <table>
        <thead>
          <tr>
            <th>任务</th>
            <th>项目</th>
            <th>状态</th>
            <th>个人安排</th>
          </tr>
        </thead>
        <tbody>
          {items.map((t) => (
            <tr
              key={t.id}
              className={t.status === "closed" ? "task-is-closed" : undefined}
            >
              <td>
                <button className="text-link" onClick={() => setEditing(t)}>
                  {t.title}
                </button>
                <small>
                  {t.source === "zentao"
                    ? "禅道" + (t.remoteType === "task" ? "任务" : "执行")
                    : "个人任务"}
                </small>
              </td>
              <td>
                {data.projects.find((p) => p.id === t.projectId)?.name ||
                  "未关联"}
              </td>
              <td>
                <span className={"tag " + t.status}>{statuses[t.status]}</span>
                {isTaskExpired(t) && (
                  <span className="tag expired">已过期</span>
                )}
              </td>
              <td>{scheduleLabel(t)}</td>
            </tr>
          ))}
        </tbody>
      </table>
      {!items.length && (
        <div className="empty">
          <CircleDashed />
          <p>暂无任务</p>
          <button onClick={() => setEditing(blankTask())}>
            <Plus size={15} />
            新建任务
          </button>
        </div>
      )}
    </div>
  );
  return (
    <>
      <div className="page-heading">
        <div>
          <h1>
            {page === "overview"
              ? "总览"
              : page === "calendar"
                ? "日历"
                : "我的任务"}
          </h1>
          <p>
            {page === "overview"
              ? DateTime.local()
                  .setLocale("zh-CN")
                  .toFormat("yyyy年M月d日 · cccc")
              : page === "calendar"
                ? "安排工作，留出时间"
                : "个人任务与禅道工作进展"}
          </p>
        </div>
        <button className="primary" onClick={() => setEditing(blankTask())}>
          <Plus size={16} />
          新建任务
        </button>
      </div>
      {page === "overview" ? (
        <>
          <div className="metrics">
            {(Object.keys(statuses) as Status[]).map((s) => {
              const Icon = statusIcons[s];
              return (
                <button key={s} onClick={() => navigate("/tasks")}>
                  <span className={s}>
                    <Icon size={17} />
                    {statuses[s]}
                  </span>
                  <strong>
                    {data.tasks.filter((t) => t.status === s).length}
                  </strong>
                </button>
              );
            })}
          </div>
          <section>
            <div className="section-heading">
              <h2>今日安排</h2>
              <button onClick={() => navigate("/calendar")}>
                <CalendarDays size={15} />
                日历
              </button>
            </div>
            {taskRows(todayTasks)}
          </section>
          <section>
            <div className="section-heading">
              <h2>BUG 修复</h2>
              <button onClick={() => navigate("/bugs")}>
                管理 BUG
                <ArrowUpRight size={14} />
              </button>
            </div>
            <p className="muted">
              指派给我的 BUG · 缓存 {bugStats.total} 条 · 严重待解决{" "}
              {bugStats.urgent} 条
            </p>
            <div className="metrics">
              {(
                [
                  ["active", "待解决", bugStats.active, "todo"],
                  ["resolved", "已解决", bugStats.resolved, "done"],
                  ["closed", "已关闭", bugStats.closed, "closed"],
                ] as const
              ).map(([status, label, count, tone]) => (
                <button
                  key={status}
                  onClick={() => navigate(`/bugs?status=${status}`)}
                >
                  <span className={tone}>{label}</span>
                  <strong>{count}</strong>
                </button>
              ))}
              <button onClick={() => navigate("/statistics?view=bugs")}>
                <span>统计分析</span>
                <strong>{bugStats.total}</strong>
              </button>
            </div>
            {!bugStats.total && (
              <p className="muted">
                进入 BUG 修复，点击同步即可读取禅道指派给你的 BUG。
              </p>
            )}
          </section>
          <section>
            <div className="section-heading">
              <h2>常用应用</h2>
              <button onClick={() => navigate("/apps")}>
                全部应用
                <ArrowUpRight size={14} />
              </button>
            </div>
            <div className="overview-app-grid">
              {data.apps
                .filter((a) => a.favorite)
                .slice(0, 4)
                .map((a) => (
                  <OverviewApp
                    key={a.id}
                    app={a}
                    category={
                      data.categories.find((c) => c.id === a.categoryId)
                        ?.name || "未分类"
                    }
                    notify={notify}
                  />
                ))}
            </div>
            {!data.apps.some((a) => a.favorite) && (
              <p className="muted">暂无收藏应用</p>
            )}
          </section>
        </>
      ) : page === "calendar" ? (
        <CalendarView
          tasks={list}
          onEdit={setEditing}
          onCreate={(schedule) => setEditing({ ...blankTask(), schedule })}
          onChange={async (task, schedule) => {
            await api.save("task", { ...task, schedule });
            await refresh();
          }}
        />
      ) : (
        <>
          <div className="toolbar">
            <div className="segmented">
              <button
                className={view === "board" ? "active" : ""}
                onClick={() => setView("board")}
              >
                <Columns3 size={15} />
                看板
              </button>
              <button
                className={view === "list" ? "active" : ""}
                onClick={() => setView("list")}
              >
                <List size={15} />
                列表
              </button>
            </div>
            <GlassSelect
              aria-label="项目筛选"
              value={project}
              onValueChange={setProject}
              options={[
                { value: "all", label: "全部项目" },
                ...data.projects.map((p) => ({ value: p.id, label: p.name })),
              ]}
            />
            <GlassSelect
              aria-label="状态筛选"
              value={status}
              onValueChange={setStatus}
              options={[
                { value: "all", label: "全部状态" },
                ...Object.entries(statuses).map(([value, label]) => ({
                  value,
                  label,
                })),
              ]}
            />
            <span className="muted">{list.length} 个任务</span>
            <button
              title="撤销上次拖动"
              className="icon-button"
              disabled={!undo || moving}
              onClick={async () => {
                if (!undo) return;
                setMoving(true);
                try {
                  await api.save("task", {
                    ...undo.before,
                    revision: undo.revision,
                    boardBeforeId: undo.beforeId,
                  });
                  setUndo(null);
                  await refresh();
                  notify("已撤销拖动");
                } catch (e) {
                  notify((e as Error).message);
                } finally {
                  setMoving(false);
                }
              }}
            >
              <Undo2 size={16} />
            </button>
          </div>
          {moveError && (
            <div className="notice" role="alert">
              <p className="error">{moveError}</p>
              {moveError.includes("HTTP 401") && (
                <button onClick={() => navigate("/settings?tab=zentao")}>
                  重新登录禅道
                </button>
              )}
              <button className="text-link" onClick={() => setMoveError("")}>
                关闭提示
              </button>
            </div>
          )}
          {view === "list" ? (
            taskRows(list)
          ) : (
            <DndContext
              onDragEnd={move}
              onDragStart={({ active }) =>
                setDragging(
                  data.tasks.find((task) => task.id === active.id) ?? null,
                )
              }
              onDragCancel={() => setDragging(null)}
              sensors={sensors}
              collisionDetection={boardCollision}
            >
              <div className="board">
                {(Object.keys(statuses) as Status[]).map((s) => (
                  <Column
                    key={s}
                    status={s}
                    count={list.filter((t) => t.status === s).length}
                  >
                    {list
                      .filter((t) => t.status === s)
                      .map((t) => (
                        <Fragment key={t.id}>
                          <BoardSlot
                            status={s}
                            beforeId={t.id}
                            active={!!dragging && !moving}
                          />
                          <Card
                            task={t}
                            disabled={moving}
                            project={
                              data.projects.find((p) => p.id === t.projectId)
                                ?.name
                            }
                            onOpen={() => setEditing(t)}
                            onRemote={
                              t.source === "zentao"
                                ? () => void openRemote(t)
                                : undefined
                            }
                          />
                        </Fragment>
                      ))}
                    <BoardSlot
                      status={s}
                      beforeId={null}
                      active={!!dragging && !moving}
                    />
                    <button
                      className="add-task"
                      onClick={() => setEditing({ ...blankTask(), status: s })}
                    >
                      <Plus size={14} />
                      添加任务
                    </button>
                  </Column>
                ))}
              </div>
              <DragOverlay dropAnimation={null}>
                {dragging && (
                  <article className="task-card drag-preview">
                    <small>移动到任意状态或卡片之间</small>
                    <strong>{dragging.title}</strong>
                  </article>
                )}
              </DragOverlay>
            </DndContext>
          )}
        </>
      )}
      {editing && (
        <TaskEditor
          task={editing}
          data={data}
          refresh={refresh}
          notify={notify}
          onClose={() => setEditing(null)}
        />
      )}
    </>
  );
}
export function Projects({
  data,
  refresh,
  notify,
  search = "",
}: PageProps & { search?: string }) {
  const navigate = useNavigate();
  const [edit, setEdit] = useState<Project | null>(null),
    [error, setError] = useState(""),
    [task, setTask] = useState<Task | null>(null);
  const taskCounts = new Map<string, Record<Status, number>>();
  const bugGroups = buildBugProjectIndex(data.projects, data.bugs);
  for (const item of data.tasks) {
    if (!item.projectId) continue;
    const counts = taskCounts.get(item.projectId) || {
      todo: 0,
      doing: 0,
      done: 0,
      closed: 0,
    };
    counts[item.status]++;
    taskCounts.set(item.projectId, counts);
  }
  const projects = data.projects
    .map((project) => {
      const clean = DOMPurify.sanitize(
        (project.description || "").replace(
          /<\/(?:p|div|li|h[1-6]|tr)>|<br\s*\/?>/gi,
          " ",
        ),
        { ALLOWED_TAGS: [], ALLOWED_ATTR: [] },
      );
      const description =
        new DOMParser()
          .parseFromString(clean, "text/html")
          .body.textContent?.replace(/\s+/g, " ")
          .trim() || "";
      return {
        project,
        description,
        counts: taskCounts.get(project.id) || {
          todo: 0,
          doing: 0,
          done: 0,
          closed: 0,
        },
      };
    })
    .filter(({ project, description }) =>
      (project.name + description + (project.owner || ""))
        .toLocaleLowerCase()
        .includes(search.trim().toLocaleLowerCase()),
    );
  async function save(e: React.FormEvent) {
    e.preventDefault();
    try {
      await api.save("project", edit!);
      await refresh();
      setEdit(null);
    } catch (e) {
      setError((e as Error).message);
    }
  }
  return (
    <>
      <div className="page-heading">
        <div>
          <h1>项目</h1>
          <p>本地项目与禅道项目</p>
        </div>
        <button
          className="primary"
          onClick={() => {
            setError("");
            setEdit({
              id: crypto.randomUUID(),
              name: "",
              description: "",
              source: "local",
              status: "todo",
              owner: "",
            });
          }}
        >
          <Plus size={16} />
          新建项目
        </button>
      </div>
      <div className="projects-table-wrap">
        {!!projects.length && (
          <table className="projects-table">
            <colgroup>
              <col className="project-main-col" />
              <col className="project-date-col" />
              <col className="project-progress-col" />
              <col className="project-bug-col" />
              <col className="project-actions-col" />
            </colgroup>
            <thead>
              <tr>
                <th scope="col">项目 / 负责人</th>
                <th scope="col">计划日期</th>
                <th scope="col">任务进度</th>
                <th scope="col">BUG 进度</th>
                <th scope="col">
                  <span className="muted">操作</span>
                </th>
              </tr>
            </thead>
            <tbody>
              {projects.map(({ project: p, description, counts }) => {
                const total =
                  counts.todo + counts.doing + counts.done + counts.closed;
                const bugKey = projectBugKey(p);
                const bugs = bugKey ? bugGroups.get(bugKey) : undefined;
                return (
                  <tr key={p.id}>
                    <td>
                      <div className="project-identity">
                        <FolderKanban
                          size={21}
                          className={
                            p.source === "zentao" ? "source-remote" : "muted"
                          }
                        />
                        <div className="project-copy">
                          <div className="project-name-line">
                            <button
                              className="project-name"
                              onClick={() => navigate("/tasks?project=" + p.id)}
                            >
                              {p.name}
                            </button>
                            {p.source === "zentao" && (
                              <button
                                className="icon-button"
                                title="在禅道中打开项目"
                                aria-label={`在禅道中打开项目：${p.name}`}
                                onClick={async () => {
                                  try {
                                    await openUrl(
                                      zentaoDetailUrl(p, data.connections),
                                    );
                                  } catch (error) {
                                    notify(
                                      error instanceof Error
                                        ? error.message
                                        : String(error),
                                    );
                                  }
                                }}
                              >
                                <ArrowUpRight size={15} />
                              </button>
                            )}
                            <span className={"tag " + p.status}>
                              {statuses[p.status]}
                            </span>
                          </div>
                          {description && (
                            <p
                              className="project-description"
                              title={description}
                            >
                              {description}
                            </p>
                          )}
                          <small>
                            {p.source === "zentao"
                              ? `禅道项目 #${p.remoteId || "—"}`
                              : "本地项目"}
                            <span aria-hidden="true"> · </span>
                            {p.owner || "未设置负责人"}
                          </small>
                        </div>
                      </div>
                    </td>
                    <td>
                      <div className="project-dates">
                        {p.source === "zentao" ? (
                          <>
                            <span>
                              <span>计划开始</span>
                              <time>{p.plannedStartDate || "未设置"}</time>
                            </span>
                            <span>
                              <span>计划结束</span>
                              <time>{p.plannedEndDate || "未设置"}</time>
                            </span>
                          </>
                        ) : (
                          <span>
                            <span>截止日期</span>
                            <time>{p.dueDate || "未设置"}</time>
                          </span>
                        )}
                      </div>
                    </td>
                    <td>
                      <button
                        className="project-progress"
                        onClick={() => navigate("/tasks?project=" + p.id)}
                        aria-label={`${p.name}：查看 ${total} 项任务`}
                      >
                        <span className="project-progress-heading">
                          <strong>{total} 项任务</strong>
                          <span>完成 {counts.done}</span>
                        </span>
                        <span
                          className="project-progress-track"
                          aria-hidden="true"
                        >
                          {(Object.keys(statuses) as Status[]).map(
                            (status) =>
                              counts[status] > 0 && (
                                <span
                                  key={status}
                                  className={`project-progress-${status}`}
                                  style={{
                                    width: `${(counts[status] / total) * 100}%`,
                                  }}
                                />
                              ),
                          )}
                        </span>
                        <span className="project-task-breakdown">
                          {total
                            ? `待做 ${counts.todo} · 正在做 ${counts.doing}${counts.closed ? ` · 关闭 ${counts.closed}` : ""}`
                            : "暂无关联任务"}
                        </span>
                      </button>
                    </td>
                    <td>
                      <button
                        className="project-progress"
                        disabled={!bugKey}
                        aria-label={`${p.name}：查看 ${bugs?.total || 0} 个 BUG`}
                        onClick={() => {
                          if (bugKey)
                            navigate(
                              `/bugs?${new URLSearchParams({ project: bugKey })}`,
                            );
                        }}
                      >
                        <span className="project-progress-heading">
                          <strong>{bugs?.total || 0} 个 BUG</strong>
                          <span>解决 {bugs?.resolved || 0}</span>
                        </span>
                        <span
                          className="project-progress-track"
                          aria-hidden="true"
                        >
                          {(["active", "resolved", "closed"] as const).map(
                            (status) =>
                              !!bugs?.[status] && (
                                <span
                                  key={status}
                                  className={`project-bug-${status}`}
                                  style={{
                                    width: `${(bugs[status] / bugs.total) * 100}%`,
                                  }}
                                />
                              ),
                          )}
                        </span>
                        <span className="project-task-breakdown">
                          {bugs?.total
                            ? `待解决 ${bugs.active} · 已关闭 ${bugs.closed}`
                            : bugKey
                              ? "暂无关联 BUG"
                              : "未关联禅道"}
                        </span>
                      </button>
                    </td>
                    <td>
                      <div className="project-row-actions">
                        <button
                          title="添加任务"
                          aria-label={`为${p.name}添加任务`}
                          className="icon-button"
                          onClick={() =>
                            setTask({ ...blankTask(), projectId: p.id })
                          }
                        >
                          <Plus size={17} />
                        </button>
                        <button
                          title="编辑项目"
                          aria-label={`编辑项目${p.name}`}
                          className="icon-button"
                          onClick={() => {
                            setError("");
                            setEdit(p);
                          }}
                        >
                          <Pencil size={16} />
                        </button>
                      </div>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        )}
      </div>
      {!projects.length && (
        <div className="empty">
          <FolderKanban />
          <p>{data.projects.length ? "没有匹配的项目" : "还没有项目"}</p>
        </div>
      )}
      {edit && (
        <Modal title="项目详情" onClose={() => setEdit(null)}>
          <form onSubmit={save} className="form-grid">
            <label>
              项目名称
              <input
                required
                value={edit.name}
                disabled={edit.source === "zentao"}
                onChange={(e) => setEdit({ ...edit, name: e.target.value })}
              />
            </label>
            <label>
              描述
              <textarea
                rows={4}
                value={edit.description}
                onChange={(e) =>
                  setEdit({ ...edit, description: e.target.value })
                }
              />
            </label>
            <label>
              负责人
              <input
                value={edit.owner}
                onChange={(e) => setEdit({ ...edit, owner: e.target.value })}
              />
            </label>
            <div className="two-fields">
              <label>
                状态
                <GlassSelect
                  aria-label="项目状态"
                  value={edit.status}
                  onValueChange={(status) =>
                    setEdit({ ...edit, status: status as Status })
                  }
                  options={Object.entries(statuses).map(([value, label]) => ({
                    value,
                    label,
                  }))}
                />
              </label>
              {edit.source !== "zentao" && (
                <label>
                  截止日期
                  <input
                    type="date"
                    value={edit.dueDate || ""}
                    onChange={(e) =>
                      setEdit({ ...edit, dueDate: e.target.value })
                    }
                  />
                </label>
              )}
            </div>
            {edit.source === "zentao" && (
              <div className="two-fields">
                <label>
                  计划开始日期
                  <input readOnly value={edit.plannedStartDate || "未设置"} />
                </label>
                <label>
                  计划结束日期
                  <input readOnly value={edit.plannedEndDate || "未设置"} />
                </label>
              </div>
            )}
            {error && (
              <p className="error" role="alert">
                {error}
              </p>
            )}
            <footer>
              {edit.revision && edit.source === "local" && (
                <button
                  type="button"
                  className="danger"
                  onClick={async () => {
                    if (!confirm("删除这个项目？关联任务存在时将禁止删除。"))
                      return;
                    try {
                      await api.remove("project", edit.id, edit.revision);
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
              <button type="submit" className="primary">
                保存
              </button>
            </footer>
          </form>
        </Modal>
      )}
      {task && (
        <TaskEditor
          task={task}
          data={data}
          refresh={refresh}
          notify={notify}
          onClose={() => setTask(null)}
        />
      )}
    </>
  );
}

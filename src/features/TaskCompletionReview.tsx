import { ArrowUpRight, CircleCheck } from "lucide-react";
import { DateTime } from "luxon";
import type { buildTaskCompletionReview } from "../lib/task-completion";
import type { Project, Task } from "../lib/types";
import "./task-completion.css";

type Review = ReturnType<typeof buildTaskCompletionReview>;

export default function TaskCompletionReview({
  review,
  projects,
  onOpen,
  onRemote,
}: {
  review: Review;
  projects: Project[];
  onOpen: (task: Task) => void;
  onRemote: (task: Task) => void;
}) {
  const projectNames = new Map(projects.map((project) => [project.id, project.name]));
  const taskList = (tasks: Task[], unknown = false) => (
    <ul className="task-completion-list">
      {tasks.map((task) => {
        const completed = !unknown && typeof task.completedAt === "number"
          ? DateTime.fromMillis(task.completedAt, { zone: review.thisWeek.start.zone })
          : null;
        return (
          <li key={task.id}>
            <CircleCheck size={17} className="task-completion-check" aria-hidden="true" />
            <div className="task-completion-content">
              <button className="task-completion-title" onClick={() => onOpen(task)}>
                {task.title}
              </button>
              <div className="task-completion-meta">
                <span>{projectNames.get(task.projectId || "") || "未关联项目"}</span>
                <span>
                  {task.source === "zentao"
                    ? `禅道执行${task.remoteId ? ` #${task.remoteId}` : ""}`
                    : "个人任务"}
                </span>
              </div>
              {completed ? (
                <time dateTime={completed.toISO() || undefined}>
                  {completed.toFormat("M月d日 HH:mm")} 完成
                </time>
              ) : (
                <span className="task-completion-date-unknown">完成时间未记录</span>
              )}
            </div>
            {task.source === "zentao" && (
              <button
                className="icon-button task-completion-remote"
                title="在禅道中打开"
                aria-label={`在禅道中打开：${task.title}`}
                onClick={() => onRemote(task)}
              >
                <ArrowUpRight size={16} />
              </button>
            )}
          </li>
        );
      })}
    </ul>
  );

  return (
    <div className="task-completion-review">
      <p className="task-completion-hint">按完成时间归周，周一至周日；最新完成的任务排在前面。</p>
      <div className="task-completion-weeks">
        {([
          ["本周完成", review.thisWeek],
          ["上周完成", review.lastWeek],
        ] as const).map(([label, week]) => {
          const lastDay = week.end.minus({ days: 1 });
          return (
            <section className="task-completion-week" key={label} aria-label={label}>
              <header>
                <div>
                  <h2>{label}<span>{week.tasks.length} 个</span></h2>
                  <p>
                    {week.start.toFormat("yyyy年M月d日")} – {lastDay.toFormat(
                      week.start.year === lastDay.year ? "M月d日" : "yyyy年M月d日",
                    )}
                  </p>
                </div>
              </header>
              {week.tasks.length ? taskList(week.tasks) : (
                <div className="task-completion-empty">
                  <CircleCheck size={24} aria-hidden="true" />
                  <p>暂无符合条件的已完成任务</p>
                </div>
              )}
            </section>
          );
        })}
      </div>
      {review.unknown.length > 0 && (
        <details className="task-completion-unknown">
          <summary>完成时间未记录 · {review.unknown.length} 个任务</summary>
          <p>这些任务已完成，但缺少可靠的完成时间，暂不计入本周或上周。</p>
          {taskList(review.unknown, true)}
        </details>
      )}
    </div>
  );
}

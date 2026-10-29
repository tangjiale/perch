import { ArrowUpRight, ListChecks, Plus } from "lucide-react";
import type { Meeting } from "../../lib/meetings";
import { taskStatuses, type Task } from "../../lib/types";

type Action = NonNullable<Meeting["analysis"]>["actionItems"][number];
export default function MeetingActions({
  meeting,
  tasks,
  disabled,
  canOpenTask,
  onComplete,
  onCreateTask,
  onOpenTask,
}: {
  meeting: Meeting;
  tasks: Task[];
  disabled: boolean;
  canOpenTask: boolean;
  onComplete: (action: Action, completed: boolean) => void;
  onCreateTask: (action: Action) => void;
  onOpenTask: (id: string) => void;
}) {
  const actions = meeting.analysis?.actionItems ?? [];
  if (!actions.length)
    return (
      <div className="meeting-empty">
        <ListChecks size={30} />
        <h3>
          {meeting.analysis
            ? "会议中未提取到明确行动项"
            : "把会议结论变成下一步行动"}
        </h3>
        <p>
          {meeting.analysis
            ? "可在会议资料中补充待核实事项，再整理纪要。"
            : "保存转录并生成纪要后，在这里核对负责人和期限。"}
        </p>
      </div>
    );
  const linkedTasks = new Map(tasks.map((task) => [task.id, task]));
  const isComplete = (action: Action) =>
    action.taskId
      ? linkedTasks.get(action.taskId)?.status === "done"
      : !!action.completed;
  const incomplete = actions.filter((action) => !isComplete(action)).length;
  return (
    <section className="meeting-actions-panel" aria-label="会议行动项">
      <div className="meeting-actions-heading">
        <h2>会后行动</h2>
        <span>
          {incomplete} 项待处理 ·{" "}
          {actions.filter((action) => !!action.taskId).length} 项已转任务
        </span>
      </div>
      <p className="meeting-hint">
        核对后可标记完成或转为本地任务。转为任务后，在“我的任务”管理状态与排期。
      </p>
      {actions.some((action) => !action.id) && (
        <p className="meeting-notice">
          这份旧纪要尚未包含行动项标识，请重新生成后管理行动项。
        </p>
      )}
      <ul className="meeting-action-list">
        {actions.map((action, index) => (
          <li
            className={isComplete(action) ? "is-complete" : undefined}
            key={action.id ?? index}
          >
            <input
              type="checkbox"
              aria-label={`${isComplete(action) ? "撤销完成" : "完成行动项"}：${action.task}`}
              checked={isComplete(action)}
              disabled={
                disabled ||
                !!meeting.analysis?.stale ||
                !action.id ||
                !!action.taskId
              }
              title={action.taskId ? "请在我的任务中管理任务状态" : undefined}
              onChange={(event) => onComplete(action, event.target.checked)}
            />
            <div className="meeting-action-copy">
              <strong>{action.task}</strong>
              <div className="meeting-action-meta">
                <span>负责人：{action.owner || "待确认"}</span>
                <span>期限：{action.deadline || "待确认"}</span>
                {!action.taskId && action.completed && <span>已完成</span>}
                {action.taskId && (
                  <span>
                    {linkedTasks.has(action.taskId)
                      ? `任务${taskStatuses[linkedTasks.get(action.taskId)!.status]}`
                      : disabled
                        ? "正在读取任务状态"
                        : "关联任务已不存在，请在我的任务中核对"}
                  </span>
                )}
              </div>
            </div>
            {action.taskId ? (
              <button
                disabled={
                  disabled || !canOpenTask || !linkedTasks.has(action.taskId)
                }
                onClick={() => onOpenTask(action.taskId!)}
              >
                <ArrowUpRight size={14} />
                打开任务
              </button>
            ) : (
              <button
                disabled={
                  disabled ||
                  !!meeting.analysis?.stale ||
                  !action.id ||
                  !!action.completed
                }
                onClick={() => onCreateTask(action)}
              >
                <Plus size={14} />
                转为任务
              </button>
            )}
          </li>
        ))}
      </ul>
    </section>
  );
}

import { useEffect, useState } from "react";
import { api } from "../lib/api";
import type { Task, ZentaoTaskAssignment } from "../lib/types";

export default function TaskAssignmentDetails({ task, disabled }: {
  task: Task;
  disabled: boolean;
}) {
  const [details, setDetails] = useState<ZentaoTaskAssignment | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [attempt, setAttempt] = useState(0);

  useEffect(() => {
    let active = true;
    setLoading(true);
    setDetails(null);
    setError("");
    api.taskAssignment(task.id).then(result => {
      if (active) setDetails(result);
    }).catch(cause => {
      if (active) setError(cause instanceof Error ? cause.message : "无法读取禅道指派信息");
    }).finally(() => {
      if (active) setLoading(false);
    });
    // 关闭或切换任务后忽略迟到的结果，指派信息不进入编辑表单。
    return () => { active = false; };
  }, [task.id, task.revision, task.connectionId, task.remoteId, attempt]);

  return <div className="task-assignment-details" aria-live="polite" aria-busy={loading}>
    {loading ? <p className="muted">正在读取指派信息…</p> : error ?
      <div className="task-assignment-error">
        <p className="muted">指派信息读取失败：{error}</p>
        <button type="button" className="subtle" disabled={disabled}
          onClick={() => setAttempt(previous => previous + 1)}>重试</button>
      </div> : details && <>
        <dl>
          <div>
            <dt>指派人</dt>
            <dd>{details.assignedBy || "未找到指派记录"}</dd>
          </div>
          {details.assignedBy && details.assignedAt && <div>
            <dt>指派时间</dt>
            <dd><time>{details.assignedAt.replace("T", " ")}</time></dd>
          </div>}
          {details.createdBy && <div>
            <dt>创建人</dt>
            <dd>{details.createdBy}</dd>
          </div>}
        </dl>
        {!details.assignedBy && <p className="muted">禅道未提供可核实的负责人变更记录。</p>}
      </>}
  </div>;
}

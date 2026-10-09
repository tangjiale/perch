import { createRoot } from "react-dom/client";
import { TaskEditor } from "../src/features/Work";
import { api, emptySnapshot } from "../src/lib/api";
import type { Task, ZentaoTaskAssignment } from "../src/lib/types";

export function mountAssignment(host: HTMLElement) {
  const root = createRoot(host);
  const original = api.taskAssignment;
  const requests: { taskId: string; resolve: (value: ZentaoTaskAssignment) => void; reject: (error: Error) => void }[] = [];
  api.taskAssignment = taskId => new Promise((resolve, reject) => {
    requests.push({ taskId, resolve, reject });
  });
  const show = (id: string, source: Task["source"] = "zentao") => {
    const task: Task = {
      id, revision: 1, title: "飞英专家小程序-优化隐患生成-后端", notes: "",
      source, remoteType: source === "zentao" ? "execution" : undefined,
      remoteId: "426", remoteStatus: "wait", status: "todo", priority: "normal",
      projectId: "p", sortOrder: 0,
      schedule: { kind: "all_day", start: "2026-10-08", end: "2026-10-09", timezone: "Asia/Shanghai" },
    };
    root.render(<TaskEditor key={id} task={task}
      data={{ ...emptySnapshot, projects: [{ id: "p", name: "飞英专家小程序", description: "", source, status: "todo", owner: "项目负责人" }] }}
      refresh={async () => {}} notify={() => {}} onClose={() => root.render(null)} />);
  };
  show("initial");
  return { requests, show, dispose: () => { root.unmount(); api.taskAssignment = original; } };
}

import React from "react";
import { createRoot } from "react-dom/client";
import { MemoryRouter } from "react-router-dom";
import Work, { TaskEditor, Projects } from "../src/features/Work";
import { api, emptySnapshot } from "../src/lib/api";
import type { Task } from "../src/lib/types";

export function mountProjectDates(host: HTMLElement) {
  const root = createRoot(host);
  root.render(<MemoryRouter><Projects data={{...emptySnapshot,projects:[{id:"p",name:"计划项目",description:"",source:"zentao",status:"todo",owner:"测试",plannedStartDate:"2026-09-21",plannedEndDate:"2026-10-22"}]}} refresh={async()=>{}} notify={()=>{}} /></MemoryRouter>);
  return ()=>root.unmount();
}

export function mountDrag(host: HTMLElement) {
  const root = createRoot(host);
  const original = api.save;
  let tasks: Task[] = Array.from({ length: 12 }, (_, i) => ({
    id: `drag-${i}`,
    title: `拖动任务${i}`,
    notes: "",
    source: "local",
    status: "todo",
    priority: "normal",
    sortOrder: i,
  }));
  const saved: Task[] = [];
  api.save = (async (_kind: string, value: Task) => {
    saved.push(value);
    tasks = tasks.map((task) =>
      task.id === value.id ? { ...value, revision: 1 } : task,
    );
    return { ...value, revision: 1 };
  }) as typeof api.save;
  const render = () =>
    root.render(
      <MemoryRouter>
        <Work
          data={{ ...emptySnapshot, tasks }}
          refresh={async () => render()}
          notify={() => {}}
        />
      </MemoryRouter>,
    );
  render();
  return {
    saved,
    dispose: () => {
      root.unmount();
      api.save = original;
    },
  };
}
export function mountExpiry(host: HTMLElement) {
  const root = createRoot(host);
  const tasks = (["todo", "doing", "done", "closed"] as const).map(
    (status, i) => ({
      id: status,
      title: `过期-${status}`,
      notes: "",
      priority: "normal" as const,
      status,
      source: "local" as const,
      sortOrder: i,
      schedule: {
        kind: "all_day" as const,
        timezone: "Asia/Shanghai",
        start: "2020-01-01",
        end: "2020-01-02",
      },
    }),
  );
  root.render(
    <MemoryRouter>
      <Work
        data={{ ...emptySnapshot, tasks }}
        refresh={async () => {}}
        notify={() => {}}
      />
    </MemoryRouter>,
  );
  return () => root.unmount();
}

export function mountRemoteEditor(host: HTMLElement) {
  const root = createRoot(host);
  root.render(
    <TaskEditor
      task={{
        id: "remote-editor",
        revision: 1,
        title: "禅道执行",
        notes: "",
        source: "zentao",
        remoteType: "execution",
        remoteId: "1",
        status: "todo",
        priority: "normal",
        sortOrder: 0,
      }}
      data={emptySnapshot}
      refresh={async () => {}}
      notify={() => {}}
      onClose={() => {}}
    />,
  );
  return () => root.unmount();
}

import React from "react";
import { createRoot } from "react-dom/client";
import { TaskEditor } from "../src/features/Work";
import { api, emptySnapshot } from "../src/lib/api";
import type { Task } from "../src/lib/types";
import "../src/styles.css";
import "../src/glass.css";
import "../src/theme.css";

const root = createRoot(document.getElementById("test-root")!);
const fixture = window as typeof window & {
  drawTask: (overrides?: Partial<Task>, refreshFails?: boolean) => void;
  calls: {
    kind: string;
    value: Task;
    resolve: (value: Task) => void;
    reject: (error: Error) => void;
  }[];
  closeCount: number;
};
fixture.calls = [];
fixture.closeCount = 0;
api.save = ((kind: string, value: Task) =>
  new Promise<Task>((resolve, reject) =>
    fixture.calls.push({ kind, value, resolve, reject }),
  )) as typeof api.save;
let serial = 0;
fixture.drawTask = (overrides = {}, refreshFails = false) =>
  root.render(
    <TaskEditor
      key={++serial}
      task={{
        id: "fixture-draft",
        title: "测试执行",
        notes: "测试描述",
        status: "doing",
        source: "local",
        priority: "normal",
        sortOrder: 1,
        schedule: {
          kind: "timed",
          timezone: "Asia/Shanghai",
          start: "2026-09-08T09:00:00+08:00",
          end: "2026-09-10T18:00:00+08:00",
        },
        ...overrides,
      }}
      data={{
        ...emptySnapshot,
        projects: [
          {
            id: "remote",
            source: "zentao",
            name: "禅道项目",
            owner: "我",
            description: "",
            status: "doing",
            connectionId: "c",
            remoteId: "1",
          },
          {
            id: "local",
            source: "local",
            name: "个人项目",
            owner: "我",
            description: "",
            status: "doing",
          },
        ],
      }}
      onClose={() => {
        fixture.closeCount++;
        root.render(null);
      }}
      refresh={async () => {
        if (refreshFails) throw Error("测试刷新失败");
      }}
      notify={() => {}}
    />,
  );
fixture.drawTask();

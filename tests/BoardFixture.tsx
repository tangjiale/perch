import React, { useState } from "react";
import { createRoot } from "react-dom/client";
import { MemoryRouter } from "react-router-dom";
import Work from "../src/features/Work";
import { api, emptySnapshot } from "../src/lib/api";
import type { Task } from "../src/lib/types";
import "../src/styles.css";
import "../src/glass.css";
import "../src/theme.css";
let tasks: Task[] = ["甲", "乙", "丙", "丁"].map((title, index) => ({
  id: String(index),
  title,
  notes: "隔离拖拽验收",
  source: "local",
  status: index === 3 ? "doing" : "todo",
  sortOrder: 10,
  priority: "normal",
  revision: 1,
}));
let fail = false;
(window as any).setBoardFailure = (value: boolean) => {
  fail = value;
};
// 仅隔离 fixture 模拟持久层，不访问真实 IPC 或禅道。
api.save = (async (
  _kind: unknown,
  value: Task & { boardBeforeId?: string | null },
) => {
  if (fail) throw new Error("模拟保存失败");
  const column = tasks
    .filter((t) => t.status === value.status && t.id !== value.id)
    .sort((a, b) => a.sortOrder - b.sortOrder || a.id.localeCompare(b.id));
  const index =
    value.boardBeforeId == null
      ? column.length
      : column.findIndex((t) => t.id === value.boardBeforeId);
  const saved = { ...value, revision: value.revision! + 1 };
  column.splice(index, 0, saved);
  column.forEach((t, i) => {
    t.sortOrder = (i + 1) * 1024;
  });
  tasks = tasks
    .filter((t) => t.id !== value.id && t.status !== value.status)
    .concat(column);
  (window as any).boardSaved = tasks;
  return saved;
}) as typeof api.save;
function Fixture() {
  const [data, setData] = useState({ ...emptySnapshot, tasks: [...tasks] });
  return (
    <MemoryRouter>
      <main style={{ padding: 24 }}>
        <Work
          data={data}
          refresh={async () => setData({ ...data, tasks: [...tasks] })}
          notify={(message) => {
            (window as any).boardNotice = message;
          }}
        />
      </main>
    </MemoryRouter>
  );
}
createRoot(document.getElementById("root")!).render(<Fixture />);

import React from "react";
import { createRoot } from "react-dom/client";
import { MemoryRouter } from "react-router-dom";
import Bugs from "../src/features/Bugs";
import { emptySnapshot } from "../src/lib/api";
import "../src/styles.css";
import "../src/glass.css";
import "../src/theme.css";

const bugs = (["active", "resolved", "closed"] as const).map(
  (status, index) => ({
    id: `bug-${index}`,
    connectionId: "test",
    remoteId: String(100 + index),
    revision: 1,
    title: `${status === "active" ? "登录页面" : status === "resolved" ? "邮件列表" : "项目日期"}测试 BUG`,
    steps:
      '<p>测试复现步骤</p><img src="https://invalid.example/track" onerror="window.__unsafeBug=true" /><a href="javascript:alert(1)">不安全链接</a><script>window.__unsafeBug=true</script>',
    status,
    assignedTo: status === "closed" ? "" : "tester",
    assignedToName: status === "closed" ? "" : "测试用户",
    lastAssignedTo: "tester",
    severity: 2,
    priority: 1,
    productName: "测试产品",
    projectName: "测试项目",
    openedDate: `2026-09-${String(8 + index).padStart(2, "0")} 09:00:00`,
  }),
);
createRoot(document.getElementById("bugs-test-root")!).render(
  <MemoryRouter initialEntries={[`/bugs${window.location.search}`]}>
    <Bugs
      data={{
        ...emptySnapshot,
        connections: [
          {
            id: "test",
            name: "测试禅道",
            baseUrl: "https://invalid.example/zentao",
            apiVersion: "v1",
            enabled: true,
            managementEnabled: true,
          },
        ],
        bugs,
      }}
      refresh={async () => {}}
      notify={() => {}}
    />
  </MemoryRouter>,
);

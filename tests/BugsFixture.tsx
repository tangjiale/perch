import React, { useState } from "react";
import { createRoot } from "react-dom/client";
import { MemoryRouter } from "react-router-dom";
import Bugs from "../src/features/Bugs";
import { emptySnapshot } from "../src/lib/api";
import type { ZentaoBug } from "../src/lib/types";
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
      index === 0
        ? '<p><img src="{708.png}" alt="复现截图" onerror="window.__unsafeBug=true" /></p>'
        : '<p>测试复现步骤</p><img src="https://invalid.example/zentao/index.php?m=file&amp;f=read&amp;fileID=709" /><img src="https://tracking.invalid/track" onerror="window.__unsafeBug=true" /><a href="javascript:alert(1)">不安全链接</a><script>window.__unsafeBug=true</script>',
    status,
    confirmed:
      index === 0
        ? new URLSearchParams(window.location.search).get("confirmed") ===
          "true"
          ? true
          : new URLSearchParams(window.location.search).get("confirmed") ===
              "unknown"
            ? null
            : false
        : index === 1
          ? true
          : null,
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
const confirmationFilterBugs: ZentaoBug[] = Array.from(
  { length: 122 },
  (_, index) => ({
    ...bugs[0],
    id: `filter-${index}`,
    remoteId: String(1000 + index),
    title: `${[12, 120, 121].includes(index) ? "边界筛选" : "筛选 BUG"} ${index}`,
    status:
      index >= 120
        ? "active"
        : (["active", "resolved", "closed"] as const)[index % 3],
    confirmed:
      index === 120 ? null : index === 121 ? undefined : index % 2 === 0,
    projectId: Math.floor(index / 2) % 2 === 0 ? "11" : "22",
    projectName: Math.floor(index / 2) % 2 === 0 ? "甲项目" : "乙项目",
    severity: (Math.floor(index / 4) % 4) + 1,
  }),
);
function Fixture() {
  const [fixtureBugs, setFixtureBugs] = useState<ZentaoBug[]>(
    new URLSearchParams(location.search).has("confirmationFilter")
      ? confirmationFilterBugs
      : bugs,
  );
  (window as unknown as { __fixtureBugs: ZentaoBug[] }).__fixtureBugs =
    fixtureBugs;
  return (
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
          bugs: fixtureBugs,
        }}
        refresh={async () => {
          const next = (window as unknown as { __fixtureBugs: ZentaoBug[] })
            .__fixtureBugs;
          setFixtureBugs([...next]);
        }}
        notify={() => {}}
      />
    </MemoryRouter>
  );
}
createRoot(document.getElementById("bugs-test-root")!).render(<Fixture />);

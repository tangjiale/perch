import React from "react";
import { createRoot } from "react-dom/client";
import { MemoryRouter } from "react-router-dom";
import Settings from "../src/features/Settings";
import { emptySnapshot } from "../src/lib/api";
import "../src/styles.css";
import "../src/glass.css";
import "../src/theme.css";
createRoot(document.getElementById("root")!).render(
  <MemoryRouter initialEntries={["/settings?tab=storage"]}>
    <main style={{ height: "100vh", overflow: "auto", padding: "28px 36px", maxWidth: 1400, margin: "auto" }}>
      <Settings data={emptySnapshot} refresh={async () => { window.dispatchEvent(new Event("fixture-restored")); }}
        info={{ dataRoot: "/mock/perch", defaultRoot: "/mock/perch", workspaceId: "fixture-workspace", version: "test" }}
        onInfo={async () => undefined} notify={(text) => { document.documentElement.dataset.notice = text; }} />
    </main>
  </MemoryRouter>,
);

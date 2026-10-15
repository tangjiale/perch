import React from "react";
import { createRoot } from "react-dom/client";
import { MemoryRouter } from "react-router-dom";
import Tools from "../src/features/Tools";
import "../src/styles.css";
import "../src/glass.css";
import "../src/theme.css";
createRoot(document.getElementById("root")!).render(
  <MemoryRouter
    initialEntries={[
      new URLSearchParams(location.search).get("page") || "/tools",
    ]}
  >
    <Tools
      info={{
        dataRoot: "/mock/perch",
        defaultRoot: "/mock/perch",
        workspaceId: "fixture",
        version: "test",
      }}
      notify={(text) => {
        (window as unknown as { __notice: string }).__notice = text;
      }}
    />
  </MemoryRouter>,
);

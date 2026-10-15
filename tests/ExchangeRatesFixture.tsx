import React from "react";
import { createRoot } from "react-dom/client";
import { MemoryRouter } from "react-router-dom";
import Tools from "../src/features/Tools";
import "../src/styles.css";
import "../src/glass.css";
import "../src/theme.css";
createRoot(document.getElementById("root")!).render(
  <MemoryRouter initialEntries={["/tools/exchange"]}>
    <Tools info={{ dataRoot: "/mock/exchange", defaultRoot: "/mock/exchange", workspaceId: "exchange-fixture", version: "test" }} notify={() => {}} />
  </MemoryRouter>,
);

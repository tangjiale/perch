import React from "react";
import { createRoot } from "react-dom/client";
import { MemoryRouter } from "react-router-dom";
import Tools from "../src/features/Tools";
import "../src/styles.css";
import "../src/glass.css";
import "../src/theme.css";
createRoot(document.getElementById("root")!).render(<MemoryRouter initialEntries={[new URLSearchParams(location.search).has("index") ? "/tools" : "/tools/prompts"]}><Tools info={{ dataRoot: "/mock/prompts", defaultRoot: "/mock/prompts", workspaceId: "prompt-fixture", version: "test" }} notify={(text) => { document.getElementById("notice")!.textContent = text; }} /></MemoryRouter>);

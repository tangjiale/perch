import React from "react";
import { createRoot } from "react-dom/client";
import { MemoryRouter } from "react-router-dom";
import Settings from "../src/features/Settings";
import { emptySnapshot } from "../src/lib/api";

export function mountSettings(
  host: HTMLElement,
  refresh: () => Promise<unknown>,
) {
  const root = createRoot(host);
  root.render(
    <MemoryRouter initialEntries={["/settings?tab=models"]}>
      <Settings
        data={{
          ...emptySnapshot,
          providers: [
            {
              id: "fixture-provider",
              name: "LOCAL（测试供应商）",
              baseUrl: "http://localhost:8000/v1",
              protocol: "openai-completions",
              enabled: true,
              revision: 1,
            },
          ],
          models: [
            {
              id: "existing",
              providerId: "fixture-provider",
              name: "已添加模型",
              remoteModelId: "existing-chat",
              capability: "chat",
              enabled: false,
              revision: 1,
            },
          ],
        }}
        refresh={refresh}
        notify={() => {}}
        onInfo={async () => {}}
        info={{
          workspaceId: "fixture",
          dataRoot: "fixture",
          defaultRoot: "fixture",
          version: "0.1.0",
        }}
      />
    </MemoryRouter>,
  );
  return () => root.unmount();
}

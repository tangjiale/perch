import React from "react";
import { createRoot } from "react-dom/client";
import { MemoryRouter } from "react-router-dom";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import Statistics from "../src/features/Statistics";
import type { Snapshot } from "../src/lib/types";

export function mountStatistics(host: HTMLElement) {
  const root = createRoot(host);
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return {
    draw(data: Snapshot) {
      root.render(
        <QueryClientProvider client={client}>
          <MemoryRouter>
            <Statistics
              data={data}
              workspaceId="fixture"
              refresh={async () => {}}
              notify={() => {}}
            />
          </MemoryRouter>
        </QueryClientProvider>,
      );
    },
    dispose() {
      root.unmount();
      client.clear();
    },
  };
}

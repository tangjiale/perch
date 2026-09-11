import React from "react";
import { createRoot } from "react-dom/client";
import { MemoryRouter } from "react-router-dom";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import Mail from "../src/features/Mail";
import MailSettings from "../src/features/MailSettings";
import GeneralSettings from "../src/features/GeneralSettings";

export function mountMailFixture(
  host: HTMLElement,
  notify: (message: string) => void,
) {
  const root = createRoot(host);
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return {
    draw(page: string) {
      root.render(
        <QueryClientProvider client={client}>
          <MemoryRouter>
            <div style={{ height: "100%" }}>
              {page === "settings" ? (
                <MailSettings />
              ) : page === "general" ? (
                <GeneralSettings />
              ) : (
                <Mail notify={notify} />
              )}
            </div>
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

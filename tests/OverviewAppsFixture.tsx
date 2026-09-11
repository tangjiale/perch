import React from "react";
import { createRoot } from "react-dom/client";
import { MemoryRouter } from "react-router-dom";
import Work from "../src/features/Work";
import { Apps } from "../src/features/Resources";
import { emptySnapshot } from "../src/lib/api";
import type { Application } from "../src/lib/types";

export function mountAppCategories(host: HTMLElement) {
  const root = createRoot(host);
  root.render(<Apps data={{...emptySnapshot,
    categories:[{id:"dev",name:"研发工具"},{id:"office",name:"办公协作"}],
    apps:[{id:"test",name:"测试应用",url:"https://example.com",description:"",favorite:false,categoryId:"dev",sortOrder:0}]
  }} refresh={async()=>{}} notify={()=>{}} />);
  return () => root.unmount();
}

export function mountOverview(host: HTMLElement, apps: Application[], categories: typeof emptySnapshot.categories = []) {
  const root = createRoot(host);
  root.render(
    <MemoryRouter>
      <Work page="overview" data={{ ...emptySnapshot, apps, categories }}
        refresh={async () => {}} notify={() => {}} />
    </MemoryRouter>,
  );
  return () => root.unmount();
}

import type { BookmarkNode, BrowserBookmarkAppInput, BrowserBookmarks } from "../lib/types";

export type BookmarkCategory = BrowserBookmarkAppInput["category"];

interface EntryRange {
  key: string;
  name: string;
  start: number;
  end: number;
}

export interface BookmarkFolder extends EntryRange {
  kind: "folder";
  children: BookmarkEntry[];
}

export interface BookmarkPage extends EntryRange {
  kind: "bookmark";
  url: string;
  group: string | null;
  browserId: string;
  profileId: string;
}

export type BookmarkEntry = BookmarkFolder | BookmarkPage;

function webAddress(value: string) {
  const raw = value.trim();
  if (/\p{Cc}/u.test(raw)) return false;
  try {
    const url = new URL(raw);
    return (url.protocol === "http:" || url.protocol === "https:") && Boolean(url.hostname)
      && !url.username && !url.password;
  } catch {
    return false;
  }
}

export function indexBookmarks(sources: BrowserBookmarks[]) {
  const pages: BookmarkPage[] = [];
  const entries = new Map<string, BookmarkEntry>();

  function visit(node: BookmarkNode, browserId: string, profileId: string): BookmarkEntry {
    const key = JSON.stringify([browserId, profileId, node.kind, node.id]);
    const name = node.name.trim() || (node.kind === "bookmark" ? node.url : "未命名文件夹");
    const start = pages.length;
    let entry: BookmarkEntry;
    if (node.kind === "folder") {
      const children = node.children.map((child) => visit(child, browserId, profileId));
      entry = { kind: "folder", key, name, children, start, end: pages.length };
    } else {
      // 用连续区间表示文件夹下的可导入网页，以同一批次执行勾选和分类。
      const supported = webAddress(node.url);
      entry = { kind: "bookmark", key, name, url: node.url, group: node.group, browserId, profileId,
        start, end: supported ? start + 1 : start };
      if (supported) pages.push(entry);
    }
    entries.set(key, entry);
    return entry;
  }

  const browsers = sources.map((browser) => ({
    ...browser,
    profiles: (browser.status === "ready" ? browser.profiles : []).map((profile) => {
      const start = pages.length;
      const nodes = profile.nodes.map((node) => visit(node, browser.id, profile.id));
      return { ...profile, start, end: pages.length, nodes };
    }),
  }));
  return { browsers, pages, entries };
}

export function defaultBookmarkCategory(group: string | null, categoryIds: ReadonlyMap<string, string>): BookmarkCategory {
  const name = group?.trim();
  if (!name) return null;
  const id = categoryIds.get(name.toLowerCase());
  return id ? { kind: "existing", id } : { kind: "name", name };
}

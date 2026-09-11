import type { Project, Task, Connection } from "./types";

/** 使用查询式路由，兼容禅道安装子目录，不携带访问令牌。 */
export function zentaoDetailUrl(
  entity: Task | Project,
  connections: Connection[],
): string {
  const connection = connections.find(
    (item) => item.id === entity.connectionId,
  );
  if (entity.source !== "zentao" || !connection)
    throw new Error("禅道连接不存在，请检查连接设置。");
  if (!entity.remoteId || !/^\d+$/.test(entity.remoteId))
    throw new Error("缺少有效的禅道编号，请重新同步。");
  const base = new URL(connection.baseUrl);
  if (!["http:", "https:"].includes(base.protocol))
    throw new Error("禅道地址必须使用 HTTP 或 HTTPS");
  base.username = "";
  base.password = "";
  base.search = "";
  base.hash = "";
  base.pathname = base.pathname
    .replace(/\/index\.php\/?$/, "/")
    .replace(/\/?$/, "/");
  const module =
    "title" in entity
      ? entity.remoteType === "task"
        ? "task"
        : "execution"
      : "project";
  const url = new URL("index.php", base);
  url.search = new URLSearchParams({
    m: module,
    f: "view",
    [`${module}ID`]: entity.remoteId,
  }).toString();
  return url.toString();
}

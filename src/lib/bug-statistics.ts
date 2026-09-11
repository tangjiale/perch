import { DateTime } from "luxon";
import type { ZentaoBug } from "./types";

export function buildBugStatistics(
  bugs: ZentaoBug[] = [],
  days = 30,
  now = Date.now(),
  zone = "local",
) {
  const end = DateTime.fromMillis(now, { zone }).startOf("day");
  const trend = Array.from({ length: days }, (_, index) => ({
    date: end.minus({ days: days - index - 1 }).toISODate()!,
    opened: 0,
    resolved: 0,
  }));
  const dates = new Map(trend.map((row) => [row.date, row]));
  const status = [
    { name: "待解决", value: 0 },
    { name: "已解决", value: 0 },
    { name: "已关闭", value: 0 },
  ];
  const severity = [1, 2, 3, 4].map((level) => ({
    name: `${level} 级`,
    value: 0,
  }));
  const products = new Map<string, { name: string; value: number }>();
  let urgent = 0;
  for (const bug of bugs) {
    status[bug.status === "active" ? 0 : bug.status === "resolved" ? 1 : 2]
      .value++;
    if (bug.status === "active" && bug.severity <= 2 && bug.severity >= 1)
      urgent++;
    if (severity[bug.severity - 1]) severity[bug.severity - 1].value++;
    const key = `${bug.connectionId}:${bug.productId || "unknown"}`;
    const product = products.get(key) || {
      name: bug.productName
        ? `${bug.productName} #${bug.productId || "—"}`
        : `产品 #${bug.productId || "未指定"}`,
      value: 0,
    };
    product.value++;
    products.set(key, product);
    // 只按远端业务日期统计；缓存创建时间不能冒充 BUG 新建日期。
    for (const [value, field] of [
      [bug.openedDate, "opened"],
      [bug.resolvedDate, "resolved"],
    ] as const) {
      if (!value || value.startsWith("0000-")) continue;
      const parsed = DateTime.fromISO(value.replace(" ", "T"), { zone });
      const row = parsed.isValid ? dates.get(parsed.toISODate()!) : undefined;
      if (row) row[field]++;
    }
  }
  return {
    total: bugs.length,
    active: status[0].value,
    resolved: status[1].value,
    closed: status[2].value,
    urgent,
    status,
    severity,
    products: [...products.values()].sort((a, b) => b.value - a.value),
    trend,
  };
}

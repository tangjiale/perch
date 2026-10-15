import { useEffect, useMemo, useState } from "react";
import DOMPurify from "dompurify";
import { DateTime } from "luxon";
import { api } from "../lib/api";
import type { ZentaoBugHistory, ZentaoBugHistoryEntry } from "../lib/types";
import "./bug-history.css";

const actions: Record<string, string> = {
  opened: "创建", created: "创建", edited: "修改", assigned: "指派",
  confirmed: "确认", resolved: "解决", closed: "关闭", activated: "激活",
  commented: "添加备注", deleted: "删除", undeleted: "恢复",
  tostory: "转为需求", tocase: "转为用例", linked: "关联", unlinked: "解除关联",
};
const fields: Record<string, string> = {
  title: "标题", steps: "复现步骤", assignedTo: "指派给", status: "状态",
  confirmed: "是否确认", severity: "严重程度", pri: "优先级", resolution: "解决方案",
  resolvedBuild: "解决版本", openedBuild: "影响版本", deadline: "截止日期",
  product: "产品", project: "项目", execution: "执行", module: "模块",
  type: "类型", os: "操作系统", browser: "浏览器", duplicateBug: "重复 BUG",
};

function historyDate(value: string | null) {
  if (!value) return "时间未提供";
  // 禅道历史时间属于远端业务时间，带时区时也保留原时区展示。
  const date = DateTime.fromISO(value.replace(" ", "T"), { setZone: true });
  return date.isValid ? date.toFormat("yyyy-MM-dd HH:mm:ss") : value;
}

function HistoryComment({ value }: { value: string }) {
  const html = useMemo(() => {
    const fragment = DOMPurify.sanitize(value, {
      ALLOWED_TAGS: ["p", "br", "div", "span", "b", "strong", "em", "i", "u", "s", "ul", "ol", "li", "pre", "code", "blockquote", "table", "tbody", "thead", "tr", "td", "th", "img"],
      ALLOWED_ATTR: ["alt", "colspan", "rowspan"],
      ALLOW_DATA_ATTR: false,
      RETURN_DOM_FRAGMENT: true,
    });
    // 历史附件未经过 BUG 正文引用核验，不让备注中的图片或链接发起外部请求。
    fragment.querySelectorAll("img").forEach(image => {
      image.replaceWith(document.createTextNode(`[${image.getAttribute("alt") || "图片"}，请在禅道查看]`));
    });
    const container = document.createElement("div");
    container.append(fragment);
    return container.innerHTML;
  }, [value]);
  return html ? <div className="bug-history-comment" dangerouslySetInnerHTML={{ __html: html }} /> : null;
}

function actionLabel(entry: ZentaoBugHistoryEntry) {
  if (entry.action.toLowerCase() === "assigned") {
    const target = entry.changes.find(change => change.field.toLowerCase() === "assignedto")?.new || entry.extra;
    return target ? `指派给 ${target}` : "指派";
  }
  return actions[entry.action.toLowerCase()] || `执行操作「${entry.action}」`;
}

export default function BugHistory({ bugId, revision }: { bugId: string; revision?: number }) {
  const [details, setDetails] = useState<ZentaoBugHistory | null>(null);
  const [error, setError] = useState("");
  const [retry, setRetry] = useState(0);
  useEffect(() => {
    let active = true;
    setDetails(null);
    setError("");
    api.bugHistory(bugId).then(result => {
      if (active) setDetails(result);
    }).catch(cause => {
      if (active) setError(cause instanceof Error ? cause.message : String(cause));
    });
    return () => { active = false; };
  }, [bugId, revision, retry]);

  return <section className="bugs-full bug-history" aria-labelledby="bug-history-title">
    <h3 id="bug-history-title">历史记录</h3>
    {error ? <div className="bug-history-error" role="alert">
      <p>历史记录加载失败：{error}</p>
      <button type="button" onClick={() => setRetry(value => value + 1)}>重新加载历史</button>
    </div> : !details ? <p className="muted" role="status">正在读取禅道历史记录…</p> : <>
      <dl className="bug-history-assignment">
        <div><dt>指派人</dt><dd>{details.assignedBy || "未找到指派记录"}</dd></div>
        <div><dt>指派时间</dt><dd>{details.assignedAt ? historyDate(details.assignedAt) : "未找到指派记录"}</dd></div>
        <div><dt>创建人</dt><dd>{details.createdBy || "未提供"}</dd></div>
      </dl>
      {details.entries.length === 0 ? <p className="muted">暂无历史记录</p> : <ol className="bug-history-list">
        {details.entries.map(entry => <li key={entry.id} className="bug-history-entry">
          <div className="bug-history-event">
            <time dateTime={entry.date?.replace(" ", "T")}>{historyDate(entry.date)}</time>
            <span>由 <strong>{entry.actor || "未知操作人"}</strong> {actionLabel(entry)}。</span>
          </div>
          {entry.comment && <HistoryComment value={entry.comment} />}
          {entry.changes.length > 0 && <details className="bug-history-changes">
            <summary>查看字段变更（{entry.changes.length}）</summary>
            <dl>{entry.changes.map((change, index) => <div key={`${change.field}:${index}`}>
              <dt>{fields[change.field] || change.field}</dt>
              <dd><span className="muted">原值：</span>{change.old || "空"}</dd>
              <dd><span className="muted">新值：</span>{change.new || "空"}</dd>
            </div>)}</dl>
          </details>}
        </li>)}
      </ol>}
    </>}
  </section>;
}

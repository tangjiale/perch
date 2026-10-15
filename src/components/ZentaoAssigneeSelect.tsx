import { useEffect, useState } from "react";
import GlassSelect from "./GlassSelect";
import { api } from "../lib/api";
import type { ZentaoAssignee } from "../lib/types";

export default function ZentaoAssigneeSelect({ bugId, value, currentName, onChange, active = true, disabled, label = "BUG 指派给" }: {
  bugId: string; value: string; currentName?: string;
  onChange: (account: string, realname: string) => void;
  active?: boolean; disabled?: boolean; label?: string;
}) {
  const [people, setPeople] = useState<ZentaoAssignee[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [retry, setRetry] = useState(0);
  useEffect(() => {
    if (!active) return;
    let current = true;
    setLoading(true); setError(""); setPeople([]);
    api.bugAssignees(bugId).then(rows => { if (current) setPeople([...rows].sort((a, b) =>
      (a.realname || a.account).localeCompare(b.realname || b.account, "zh-CN") || a.account.localeCompare(b.account)
    )); })
      .catch(cause => { if (current) setError(cause instanceof Error ? cause.message : "人员列表加载失败"); })
      .finally(() => { if (current) setLoading(false); });
    return () => { current = false; };
  }, [bugId, active, retry]);
  const labelFor = (account: string, name?: string) => name && name !== account ? `${name}（${account}）` : account;
  const options = people.map(person => ({ value: person.account, label: labelFor(person.account, person.realname), disabled: false }));
  // 加载失败或当前人员已不在目录中时，保留已有指派，绝不自动选择其他账号。
  if (value && !people.some(person => person.account === value)) options.unshift({ value, label: labelFor(value, currentName), disabled: true });
  return <div className="zentao-assignee-field">
    <GlassSelect searchable aria-label={label} searchPlaceholder="搜索姓名或账号" required options={options} value={value}
      disabled={disabled || !active || loading || !!error} placeholder={loading ? "正在加载人员…" : "请选择指派人"}
      onValueChange={account => onChange(account, people.find(person => person.account === account)?.realname || account)} />
    {active && loading && <span className="zentao-assignee-status" role="status">正在加载禅道人员…</span>}
    {active && error && <div className="zentao-assignee-status" role="alert"><span>人员列表加载失败：{error}</span><button type="button" disabled={disabled} onClick={() => setRetry(value => value + 1)}>重新加载人员</button></div>}
  </div>;
}

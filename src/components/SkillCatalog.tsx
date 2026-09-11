import { useState, type ReactNode } from "react";
import { ChevronDown, ChevronRight } from "lucide-react";
import type { Skill } from "../lib/types";
import { skillSource, skillSources } from "../lib/skill-sources";
import GlassSelect from "./GlassSelect";
import "./skill-catalog.css";

export default function SkillCatalog({ skills, search, busy, renderSkill, onToggleGroup }: {
  skills: Skill[]; search: string; busy: boolean;
  renderSkill: (skill: Skill) => ReactNode;
  onToggleGroup: (skills: Skill[], enabled: boolean) => Promise<void>;
}) {
  const [query, setQuery] = useState("");
  const [source, setSource] = useState("all");
  const [expanded, setExpanded] = useState<Record<string, boolean>>({});
  const words = `${search} ${query}`.toLocaleLowerCase().trim().split(/\s+/).filter(Boolean);
  const enabledCount = skills.filter((skill) => skill.enabled).length;
  const groups = skillSources.map((group) => {
    const all = skills.filter((skill) => skillSource(skill) === group.id);
    const matches = all.filter((skill) => words.every((word) => `${skill.name} ${skill.description}`.toLocaleLowerCase().includes(word)));
    return { ...group, all, matches };
  }).filter((group) => (source === "all" || source === group.id) && (!words.length || group.matches.length > 0));
  return <>
    <div className="skill-catalog-summary">
      <span><strong>{skills.length}</strong> 个技能</span>
      <span><strong>{enabledCount}</strong> 个已启用</span>
      <span><strong>{skills.length - enabledCount}</strong> 个已停用</span>
    </div>
    <div className="skill-catalog-filters">
      <input aria-label="搜索技能名称或描述" placeholder="搜索技能名称或描述" value={query} onChange={(event) => setQuery(event.target.value)} />
      <GlassSelect aria-label="技能来源" value={source} onValueChange={setSource} options={[{value:"all",label:"全部来源"}, ...skillSources.map((group) => ({value:group.id,label:group.name}))]} />
    </div>
    <div className="skill-catalog-groups">
      {groups.map((group) => {
        const enabled = group.all.filter((skill) => skill.enabled).length;
        const allEnabled = group.all.length > 0 && enabled === group.all.length;
        const open = expanded[group.id] ?? (words.length > 0 || source !== "all");
        return <section className="skill-source-group" key={group.id}>
          <div className="skill-source-heading">
            <button className="skill-source-expand" type="button" aria-expanded={open} aria-controls={`skill-source-${group.id}`} onClick={() => setExpanded({...expanded,[group.id]:!open})}>
              {open ? <ChevronDown size={16}/> : <ChevronRight size={16}/>}
              <strong>{group.name}</strong><span>{group.all.length} 个技能</span>
              {words.length > 0 && <small>匹配 {group.matches.length} 项</small>}
            </button>
            <span className="muted">{!group.all.length ? "暂无技能" : !enabled ? "已停用" : allEnabled ? "已启用" : `部分启用 ${enabled}/${group.all.length}`}</span>
            <button type="button" role="switch" aria-checked={allEnabled} aria-label={`${group.name}整组启用状态`} className="model-status-switch" disabled={busy || !group.all.length}
              title={allEnabled ? "停用该来源的全部技能" : "启用该来源的全部技能"}
              onClick={() => void onToggleGroup(group.all, !allEnabled)}>
              <span className="model-switch-track" aria-hidden="true"><span/></span>
            </button>
          </div>
          {open && <div id={`skill-source-${group.id}`} className="skill-source-content">
            {group.matches.length ? <div className="resource-grid">{group.matches.map(renderSkill)}</div> : <p className="muted">该来源暂无技能</p>}
          </div>}
        </section>;
      })}
      {!groups.length && <p className="empty">没有匹配的技能</p>}
    </div>
  </>;
}

import type { Skill } from "./types";

export const skillSources = [
  { id: "perch", name: "栖点技能" },
  { id: "agents", name: "公共 Agent" },
  { id: "codex", name: "Codex" },
  { id: "claude", name: "Claude" },
  { id: "local", name: "其他本机技能" },
] as const;

export function skillSource(skill: Skill): string {
  if (skill.sourceGroup && skillSources.some((group) => group.id === skill.sourceGroup)) return skill.sourceGroup;
  const path = skill.sourcePath?.replaceAll("\\", "/") ?? "";
  if (!path) return skill.source === "local" ? "local" : "perch";
  if (path.includes("/.agents/skills/")) return "agents";
  if (path.includes("/.codex/skills/")) return "codex";
  if (path.includes("/.claude/skills/")) return "claude";
  return "local";
}

export function canEditSkill(skill: Skill): boolean {
  return skill.source !== "local" && !skill.sourcePath && (!skill.sourceGroup || skill.sourceGroup === "perch");
}

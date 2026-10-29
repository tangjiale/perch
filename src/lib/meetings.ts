export interface MeetingSegment {
  id: string;
  startMs: number;
  endMs: number;
  text: string;
  speaker?: string;
}
export interface MindMapNode {
  label: string;
  children?: MindMapNode[];
}
export interface MeetingContext {
  background: string;
  participants: string[];
  notes: string;
  glossary: string[];
  summaryStyle: "standard" | "brief" | "detailed";
}
export interface MeetingAction {
  id?: string;
  task: string;
  owner?: string;
  deadline?: string;
  completed?: boolean;
  taskId?: string;
}
export interface MeetingMarker {
  id: string;
  atMs: number;
  text: string;
}
export function defaultMeetingContext(): MeetingContext {
  return {
    background: "",
    participants: [],
    notes: "",
    glossary: [],
    summaryStyle: "standard",
  };
}
export interface Meeting {
  id: string;
  revision: number;
  title: string;
  createdAt: number;
  updatedAt: number;
  durationMs: number;
  audioChunks: number;
  segments: MeetingSegment[];
  speakerNames: Record<string, string>;
  transcriptionModelId?: string;
  context?: MeetingContext;
  markers?: MeetingMarker[];
  analysis?: {
    stale?: boolean;
    summary: string;
    decisions: string[];
    background?: string;
    participants?: string[];
    actionItems: MeetingAction[];
    mindMap: MindMapNode;
    modelId: string;
    generatedAt: number;
  };
}
export function makeMeeting(): Meeting {
  return {
    id: crypto.randomUUID(),
    revision: 0,
    title: "未命名会议",
    createdAt: Date.now(),
    updatedAt: Date.now(),
    durationMs: 0,
    audioChunks: 0,
    segments: [],
    speakerNames: {},
    context: defaultMeetingContext(),
    markers: [],
  };
}
export function meetingTime(ms: number): string {
  const s = Math.floor(Math.max(0, ms) / 1000);
  return `${Math.floor(s / 60)
    .toString()
    .padStart(2, "0")}:${(s % 60).toString().padStart(2, "0")}`;
}
export function meetingMarkdown(m: Meeting): string {
  const a = m.analysis;
  const context = m.context ?? defaultMeetingContext();
  const background = a?.background || context.background;
  const participants = a?.participants?.length
    ? a.participants
    : context.participants;
  return [
    `# ${m.title}`,
    a?.stale
      ? "> 旧版本纪要：转录或录音已更新，请重新生成。以下纪要与脑图尚未反映最新内容。"
      : "",
    background ? `## 会议背景\n${background}` : "",
    participants.length ? `## 参会人\n${participants.join("、")}` : "",
    context.notes ? `## 补充笔记\n${context.notes}` : "",
    m.markers?.length
      ? `## 重点记录\n${[...m.markers]
          .sort((a, b) => a.atMs - b.atMs)
          .map((marker) => `- ${meetingTime(marker.atMs)} ${marker.text}`)
          .join("\n")}`
      : "",
    a
      ? `## 摘要\n${a.summary}\n\n## 会议结论\n${a.decisions.map((x) => `- ${x}`).join("\n")}\n\n## 行动项\n${a.actionItems.map((x) => `- ${x.task}${x.owner ? `（负责人：${x.owner}）` : ""}${x.deadline ? `；期限：${x.deadline}` : ""}${x.completed ? "；已完成" : ""}${x.taskId ? "；已转为本地任务" : ""}`).join("\n")}`
      : "",
    "## 转录",
    ...[...m.segments]
      .sort((a, b) => a.startMs - b.startMs)
      .map(
        (s) =>
          `${meetingTime(s.startMs)} ${s.speaker ? m.speakerNames[s.speaker] || s.speaker : "未标记说话人"}：${s.text}`,
      ),
  ]
    .filter(Boolean)
    .join("\n\n");
}
export interface MindMapLayout {
  nodes: {
    label: string;
    lines: string[];
    height: number;
    x: number;
    y: number;
    id: number;
  }[];
  edges: { from: number; to: number }[];
  width: number;
  height: number;
}
/** 按字符的可见宽度折行，导出保留全部文字及显式换行。 */
export function mindMapLines(label: string): string[] {
  const lines: string[] = [];
  let line = "",
    units = 0;
  for (const char of label) {
    if (char === "\n") {
      lines.push(line);
      line = "";
      units = 0;
      continue;
    }
    const width = /[\u0020-\u007e]/.test(char) ? 0.6 : 1;
    if (units + width > 12 && line) {
      lines.push(line);
      line = "";
      units = 0;
    }
    line += char;
    units += width;
  }
  lines.push(line);
  return lines;
}
export function layoutMindMap(root: MindMapNode): MindMapLayout {
  const nodes: MindMapLayout["nodes"] = [],
    edges: MindMapLayout["edges"] = [];
  type Measured = {
    node: MindMapNode;
    lines: string[];
    height: number;
    subtreeHeight: number;
    children: Measured[];
  };
  function measure(node: MindMapNode): Measured {
    const lines = mindMapLines(node.label),
      height = Math.max(46, lines.length * 18 + 16),
      children = (node.children || []).map(measure);
    return {
      node,
      lines,
      height,
      children,
      subtreeHeight: Math.max(
        height,
        children.reduce((sum, child) => sum + child.subtreeHeight, 0) +
          Math.max(0, children.length - 1) * 22,
      ),
    };
  }
  let maxDepth = 0;
  function place(m: Measured, depth: number, top: number, parent?: number) {
    const id = nodes.length;
    maxDepth = Math.max(maxDepth, depth);
    nodes.push({
      label: m.node.label,
      lines: m.lines,
      height: m.height,
      x: depth * 230 + 20,
      y: top + (m.subtreeHeight - m.height) / 2,
      id,
    });
    if (parent !== undefined) edges.push({ from: parent, to: id });
    const childrenHeight =
      m.children.reduce((sum, c) => sum + c.subtreeHeight, 0) +
      Math.max(0, m.children.length - 1) * 22;
    let childTop = top + (m.subtreeHeight - childrenHeight) / 2;
    for (const child of m.children) {
      place(child, depth + 1, childTop, id);
      childTop += child.subtreeHeight + 22;
    }
  }
  const measured = measure(root);
  place(measured, 0, 20);
  return {
    nodes,
    edges,
    width: (maxDepth + 1) * 230 + 20,
    height: Math.max(130, measured.subtreeHeight + 40),
  };
}
const escapeXml = (s: string) =>
  s.replace(
    /[<>&"']/g,
    (c) =>
      ({
        "<": "&lt;",
        ">": "&gt;",
        "&": "&amp;",
        '"': "&quot;",
        "'": "&apos;",
      })[c]!,
  );
export function mindMapSvg(root: MindMapNode): string {
  const l = layoutMindMap(root);
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${l.width}" height="${l.height}" viewBox="0 0 ${l.width} ${l.height}"><rect width="100%" height="100%" fill="#fafafa"/>${l.edges
    .map((e) => {
      const a = l.nodes[e.from],
        b = l.nodes[e.to],
        ay = a.y + a.height / 2,
        by = b.y + b.height / 2;
      return `<path d="M${a.x + 190},${ay} C${a.x + 210},${ay} ${b.x - 20},${by} ${b.x},${by}" fill="none" stroke="#8c91a0"/>`;
    })
    .join(
      "",
    )}${l.nodes.map((n) => `<g><title>${escapeXml(n.label)}</title><rect x="${n.x}" y="${n.y}" width="190" height="${n.height}" rx="10" fill="#eef0f8"/><text font-family="sans-serif" font-size="13" fill="#252535">${n.lines.map((line, i) => `<tspan x="${n.x + 10}" y="${n.y + 21 + i * 18}">${escapeXml(line)}</tspan>`).join("")}</text></g>`).join("")}</svg>`;
}

/** 实时分片内的供应商编号不能伪装为跨片段统一身份。 */
export function speakerLabel(key: string, segments: MeetingSegment[]): string {
  if (key.startsWith("chunk:")) {
    const scope = key.split(":").slice(0, 2).join(":");
    const scopes = [
      ...new Set(
        segments
          .map((s) =>
            s.speaker?.startsWith("chunk:")
              ? s.speaker.split(":").slice(0, 2).join(":")
              : "",
          )
          .filter(Boolean),
      ),
    ];
    return `片段 ${scopes.indexOf(scope) + 1} · 发言人 ${key.split(":").slice(2).join(":")}`;
  }
  if (key.startsWith("part:")) {
    const [, part, ...speaker] = key.split(":");
    return `片段 ${part} · 发言人 ${speaker.join(":")}`;
  }
  return `发言人 ${key}`;
}

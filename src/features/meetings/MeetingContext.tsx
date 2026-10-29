import { useEffect, useState } from "react";
import { ChevronDown, NotebookPen } from "lucide-react";
import type { Meeting } from "../../lib/meetings";

type Context = NonNullable<Meeting["context"]>;
const terms = (text: string) => [
  ...new Set(
    text
      .split(/[\n,，;；]+/)
      .map((value) => value.trim())
      .filter(Boolean),
  ),
];

export default function MeetingContext({
  meeting,
  value,
  disabled,
  onChange,
}: {
  meeting: Meeting;
  value: Context;
  disabled: boolean;
  onChange: (value: Context) => void;
}) {
  const [participants, setParticipants] = useState(
    value.participants.join("\n"),
  );
  const [glossary, setGlossary] = useState(value.glossary.join("\n"));
  useEffect(() => {
    setParticipants(value.participants.join("\n"));
    setGlossary(value.glossary.join("\n"));
  }, [meeting.id, meeting.revision]);
  return (
    <details className="meeting-context">
      <summary>
        <NotebookPen size={15} />
        <strong>会议资料</strong>
        <span>
          {value.participants.length || value.glossary.length
            ? `${value.participants.length} 位参会人 · ${value.glossary.length} 个热词`
            : "背景、参会人、笔记与热词"}
        </span>
        <ChevronDown className="meeting-context-chevron" size={15} />
      </summary>
      <div className="meeting-context-fields">
        <label className="meeting-context-wide">
          会议背景
          <textarea
            aria-label="会议背景"
            disabled={disabled}
            value={value.background}
            rows={2}
            maxLength={5000}
            placeholder="这场会议要讨论什么、希望解决什么问题"
            onChange={(event) =>
              onChange({ ...value, background: event.target.value })
            }
          />
        </label>
        <label>
          参会人
          <textarea
            aria-label="参会人"
            disabled={disabled}
            value={participants}
            rows={3}
            placeholder="每行一人，或用逗号分隔"
            onChange={(event) => {
              setParticipants(event.target.value);
              onChange({ ...value, participants: terms(event.target.value) });
            }}
          />
          <small>用于整理纪要；发言人仍需根据实际转录核对姓名。</small>
        </label>
        <label>
          会议热词
          <textarea
            aria-label="会议热词"
            disabled={disabled}
            value={glossary}
            rows={3}
            placeholder="产品名、项目名、人名；每行一个"
            onChange={(event) => {
              setGlossary(event.target.value);
              onChange({ ...value, glossary: terms(event.target.value) });
            }}
          />
          <small>
            用于普通分片、会后转录与纪要；oMLX 实时接口暂不支持热词。
          </small>
        </label>
        <label className="meeting-context-wide">
          会后笔记
          <textarea
            aria-label="会议笔记"
            disabled={disabled}
            value={value.notes}
            rows={3}
            maxLength={20000}
            placeholder="记录需要补充的背景、重点和待核实事项"
            onChange={(event) =>
              onChange({ ...value, notes: event.target.value })
            }
          />
          <small>保存后参与纪要整理；修改资料后需重新生成纪要。</small>
        </label>
      </div>
    </details>
  );
}

import { describe, expect, it } from "vitest";
import { pcmWav } from "./meeting-recorder";
import {
  layoutMindMap,
  makeMeeting,
  meetingMarkdown,
  meetingTime,
  mindMapSvg,
  mindMapLines,
  defaultMeetingContext,
} from "./meetings";
describe("会议录音编码", () => {
  it("将48k麦克风输入转为独立16k单声道PCM WAV", () => {
    const wav = pcmWav(new Float32Array(48000).fill(0.5), 48000),
      v = new DataView(wav.buffer);
    expect(new TextDecoder().decode(wav.slice(0, 4))).toBe("RIFF");
    expect(new TextDecoder().decode(wav.slice(8, 12))).toBe("WAVE");
    expect(v.getUint32(4, true)).toBe(wav.length - 8);
    expect(v.getUint32(24, true)).toBe(16000);
    expect(v.getUint16(22, true)).toBe(1);
    expect(v.getUint16(34, true)).toBe(16);
    expect(v.getUint32(40, true)).toBe(32000);
    expect(v.getInt16(44, true)).toBe(16383);
  });
  it("钳制越界振幅，尾部分片补齐毫秒且不丢音频", () => {
    const wav = pcmWav(new Float32Array([2, -2, 0.25]), 16000),
      v = new DataView(wav.buffer);
    expect(v.getInt16(44, true)).toBe(32767);
    expect(v.getInt16(46, true)).toBe(-32768);
    expect(v.getInt16(48, true)).toBe(8191);
    expect((wav.length - 44) % 32).toBe(0);
    expect(v.getInt16(wav.length - 2, true)).toBe(0);
  });
});

describe("会议资料与行动项导出", () => {
  it("会议草稿的资料和重点记录相互独立，旧记录可使用空资料", () => {
    const first = makeMeeting();
    const second = makeMeeting();
    first.context!.participants.push("林晓");
    first.markers!.push({ id: "marker", atMs: 12000, text: "确认发布范围" });
    expect(second.context).toEqual(defaultMeetingContext());
    expect(second.markers).toEqual([]);
    delete second.context;
    delete second.markers;
    expect(meetingMarkdown(second)).not.toContain("undefined");
  });
  it("导出背景、参会人、人工笔记、按时间的重点及行动项状态，保留完整逐字稿", () => {
    const meeting = makeMeeting();
    meeting.context = {
      background: "讨论发布计划",
      participants: ["林晓", "陈舟"],
      notes: "会后补充：先验证兼容性。",
      glossary: ["Perch"],
      summaryStyle: "detailed",
    };
    meeting.markers = [
      { id: "later", atMs: 65000, text: "确认负责人" },
      { id: "first", atMs: 12000, text: "确认范围" },
    ];
    meeting.segments = [
      {
        id: "later",
        startMs: 65000,
        endMs: 67000,
        text: "后一句",
        speaker: "speaker_1",
      },
      {
        id: "first",
        startMs: 12000,
        endMs: 15000,
        text: "先一句",
        speaker: "speaker_0",
      },
    ];
    meeting.speakerNames = { speaker_0: "林晓", speaker_1: "陈舟" };
    meeting.analysis = {
      summary: "安排下周发布",
      decisions: ["先做兼容验证"],
      actionItems: [
        {
          id: "a",
          task: "核对兼容性",
          owner: "陈舟",
          deadline: "下周三",
          taskId: "task-local",
        },
        { id: "b", task: "确认范围", completed: true },
        { id: "c", task: "整理文档" },
      ],
      mindMap: { label: "发布计划" },
      modelId: "model",
      generatedAt: 1,
    };
    const markdown = meetingMarkdown(meeting);
    expect(markdown).toContain("## 会议背景\n讨论发布计划");
    expect(markdown).toContain("## 参会人\n林晓、陈舟");
    expect(markdown).toContain("## 补充笔记\n会后补充：先验证兼容性。");
    expect(markdown.indexOf("00:12 确认范围")).toBeLessThan(
      markdown.indexOf("01:05 确认负责人"),
    );
    expect(markdown).toContain(
      "核对兼容性（负责人：陈舟）；期限：下周三；已转为本地任务",
    );
    expect(markdown).toContain("确认范围；已完成");
    expect(markdown).not.toContain("2026-");
    expect(markdown.indexOf("00:12 林晓：先一句")).toBeLessThan(
      markdown.indexOf("01:05 陈舟：后一句"),
    );
    expect(meeting.segments[0].id).toBe("later");
    expect(meeting.markers[0].id).toBe("later");
  });
});
describe("会议成果", () => {
  it("导出保留真实说话人、时间和行动项，不填造负责人", () => {
    const m = makeMeeting();
    m.title = "周会";
    m.segments = [
      { id: "s", startMs: 65000, endMs: 67000, text: "下周发布", speaker: "a" },
    ];
    m.speakerNames.a = "张三";
    m.analysis = {
      summary: "讨论发布",
      decisions: ["下周发布"],
      actionItems: [{ task: "验证版本" }],
      mindMap: { label: "周会" },
      modelId: "m",
      generatedAt: 1,
    };
    const markdown = meetingMarkdown(m);
    expect(markdown).toContain("01:05 张三：下周发布");
    expect(markdown).toContain("- 验证版本");
    expect(markdown).not.toContain("负责人：undefined");
  });
  it("脑图布局具有树连接且导出转义不可信文本", () => {
    const root = {
      label: '<script>&"',
      children: [
        { label: "结论" },
        { label: "行动", children: [{ label: "上线" }] },
      ],
    };
    const layout = layoutMindMap(root);
    expect(layout.nodes).toHaveLength(4);
    expect(layout.edges).toHaveLength(3);
    expect(layout.nodes[1].y).toBeLessThan(layout.nodes[3].y);
    const svg = mindMapSvg(root);
    expect(svg).not.toContain("<script>");
    expect(svg).toContain("&lt;script&gt;&amp;&quot;");
  });
  it("时间无结束后截断且草稿ID稳定独立", () => {
    expect(meetingTime(3661000)).toBe("61:01");
    expect(meetingTime(-1)).toBe("00:00");
    expect(makeMeeting().id).not.toBe(makeMeeting().id);
  });
});

describe("长会议脑图", () => {
  it("完整保留长中文、中英文和特殊字符，自适应高度且节点不重叠", () => {
    const label =
      "会议讨论需要完整保留全部细节不能在导出中静默截断中文内容".repeat(4) +
      ' API <script>&"<> End';
    const root = { label: "会议", children: [{ label }, { label: "短节点" }] };
    const lines = mindMapLines(label);
    expect(lines.join("")).toBe(label);
    expect(lines.length).toBeGreaterThan(8);
    const layout = layoutMindMap(root),
      long = layout.nodes[1],
      next = layout.nodes[2];
    expect(long.height).toBeGreaterThan(160);
    expect(next.y).toBeGreaterThan(long.y + long.height);
    const svg = mindMapSvg(root);
    expect(svg).not.toContain("<script>");
    expect(svg).toContain("&lt;script&gt;&amp;&quot;&lt;&gt; End");
    expect(svg).toContain("<tspan");
    expect(svg).toContain(`height="${long.height}"`);
  });
});

it("旧版本纪要导出明确标记且保留现有成果", () => {
  const meeting = makeMeeting();
  meeting.analysis = {
    stale: true,
    summary: "旧版摘要",
    decisions: [],
    actionItems: [],
    mindMap: { label: "旧脑图" },
    modelId: "model",
    generatedAt: 1,
  };
  const markdown = meetingMarkdown(meeting);
  expect(markdown).toContain("旧版本纪要");
  expect(markdown).toContain("转录或录音已更新，请重新生成");
  expect(markdown).toContain("旧版摘要");
});

import { createRoot } from "react-dom/client";
import { useState } from "react";
import type { Task } from "../src/lib/types";
import Meetings from "../src/features/Meetings";
import { emptySnapshot } from "../src/lib/api";
import "../src/styles.css";
import "../src/glass.css";
import "../src/theme.css";
const base = { revision: 1, createdAt: 1, updatedAt: 1 };
function Fixture() {
  const fixture = window as unknown as {
    __tasks: Task[];
    __taskRefreshFail?: boolean;
    __taskRefreshCount?: number;
    __openedTask?: string;
  };
  const [tasks, setTasks] = useState<Task[]>(fixture.__tasks ?? []);
  return (
    <Meetings
      workspaceId="fixture"
      notify={(text) => console.info(text)}
      onTasksChanged={async () => {
        if (fixture.__taskRefreshFail) throw new Error("模拟任务列表刷新失败");
        fixture.__taskRefreshCount = (fixture.__taskRefreshCount || 0) + 1;
        setTasks(structuredClone(fixture.__tasks));
      }}
      onOpenTask={(id) => {
        fixture.__openedTask = id;
      }}
      data={{
        ...emptySnapshot,
        tasks,
        providers: [
          {
            ...base,
            id: "p",
            name: "模拟服务",
            baseUrl: "https://fixture.invalid/v1",
            protocol: "openai-completions",
            enabled: true,
          },
        ],
        models: [
          {
            ...base,
            id: "speech",
            name: "Whisper 本地实时转录",
            providerId: "p",
            remoteModelId: "speech-model",
            enabled: true,
            capability: "speech",
            speechMode: "transcription",
          },
          {
            ...base,
            id: "speech-diarized",
            name: "支持说话人识别的服务",
            providerId: "p",
            remoteModelId: "diarization-model",
            enabled: true,
            capability: "speech",
            speechMode: "diarization",
          },
          {
            ...base,
            id: "chat",
            name: "会议纪要模型",
            providerId: "p",
            remoteModelId: "chat-model",
            enabled: true,
            capability: "chat",
          },
        ],
      }}
    />
  );
}
createRoot(document.getElementById("root")!).render(<Fixture />);

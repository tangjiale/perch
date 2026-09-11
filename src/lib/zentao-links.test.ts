import { expect, it } from "vitest";
import { zentaoDetailUrl } from "./zentao-links";
import type { Connection, Project, Task } from "./types";
const connection = {
  id: "c",
  baseUrl: "https://zentao.example/zentao/index.php?m=my#x",
} as Connection;
const project = {
  source: "zentao",
  connectionId: "c",
  remoteId: "12",
  name: "项目",
} as Project;
it("项目与执行跳转保留安装目录且不携带敏感参数", () => {
  expect(zentaoDetailUrl(project, [connection])).toBe(
    "https://zentao.example/zentao/index.php?m=project&f=view&projectID=12",
  );
  expect(
    zentaoDetailUrl(
      { ...project, title: "任务", remoteType: "execution" } as unknown as Task,
      [connection],
    ),
  ).toContain("m=execution&f=view&executionID=12");
  expect(
    zentaoDetailUrl(
      { ...project, title: "任务", remoteType: "task" } as unknown as Task,
      [connection],
    ),
  ).toContain("m=task&f=view&taskID=12");
});
it("拒绝缺失关联和不安全协议", () => {
  expect(() => zentaoDetailUrl(project, [])).toThrow("连接不存在");
  expect(() =>
    zentaoDetailUrl(project, [{ ...connection, baseUrl: "file:///tmp" }]),
  ).toThrow("HTTP");
});

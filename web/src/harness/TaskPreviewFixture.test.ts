import { expect, test } from "vitest";
import { taskPreviewFixtureResponse } from "./TaskPreviewFixture";

const prefix = "/api/v1/integrations/jira/task-links/preview-fixture";

test("owns only fictional task reads and exposes eight labeled previews", async () => {
  expect(taskPreviewFixtureResponse("/api/v1/tasks/real-task")).toBeUndefined();
  const response = await taskPreviewFixtureResponse(`${prefix}/detail`);
  const detail = await response!.json();
  expect(detail.attachments).toHaveLength(8);
  expect(detail.attachments.every((item: { filename: string }) => item.filename.startsWith("Fictional preview"))).toBe(true);
  expect((await taskPreviewFixtureResponse(`${prefix}/attachments/1`))!.headers.get("Content-Type")).toBe("image/svg+xml");
});

test.each([false, true])("the held fixture read rejects cancellation (already aborted: %s)", async (alreadyAborted) => {
  const controller = new AbortController();
  if (alreadyAborted) controller.abort();
  const response = taskPreviewFixtureResponse(`${prefix}/attachments/0`, { signal: controller.signal });
  const rejected = expect(response).rejects.toMatchObject({ name: "AbortError" });
  controller.abort();
  await rejected;
});

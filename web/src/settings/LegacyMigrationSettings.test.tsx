import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import LegacyMigrationSettings from "./LegacyMigrationSettings";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

test("previews before importing and selects only server-approved records", async () => {
  const requests: Array<{ url: string; body: unknown }> = [];
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    requests.push({ url, body: init?.body ? JSON.parse(String(init.body)) : undefined });
    if (url.endsWith("/api/v1/migrations/legacy/tasks")) return ok([]);
    if (url.endsWith("/preview")) {
      return ok({
        bundle_digest: "preview-digest",
        source_installation_id: "legacy-hive",
        selectable: 1,
        skipped: 1,
        invalid: 0,
        records: [
          {
            source_id: "local-1",
            title: "Finish local work",
            source_status: "active",
            target_state: "ready",
            priority: "normal",
            matched_worker_id: "worker-1",
            matched_worker_name: "Clover",
            disposition: "transformed",
            selectable: true,
            warnings: ["Legacy Active becomes Ready because no running Legacy process is transferred."],
          },
          {
            source_id: "jira-1",
            title: "Jira work",
            source_status: "assigned",
            priority: "normal",
            disposition: "skipped_jira",
            selectable: false,
            warnings: ["Jira remains canonical; this issue will return through Jira sync."],
          },
        ],
      });
    }
    if (url.endsWith("/commit")) {
      return ok({
        batch_id: "batch-1",
        bundle_digest: "preview-digest",
        source_installation_id: "legacy-hive",
        source_snapshot_digest: "source",
        imported_task_ids: ["task-next-1"],
        imported_source_ids: ["local-1"],
        imported_at: 123,
      }, 201);
    }
    throw new Error(`unexpected ${url}`);
  }));

  const { container } = render(<LegacyMigrationSettings busy={false} operatorToken="token" />);
  const file = new File([JSON.stringify({
    format: "swarm-next-migration",
    version: 1,
    source: { installation_id: "legacy-hive", exported_at: 1, snapshot_digest: "source" },
    tasks: [{ source_id: "local-1" }],
  })], "legacy.json", { type: "application/json" });
  Object.defineProperty(file, "text", {
    value: async () => JSON.stringify({
      format: "swarm-next-migration",
      version: 1,
      source: { installation_id: "legacy-hive", exported_at: 1, snapshot_digest: "source" },
      tasks: [{ source_id: "local-1" }],
    }),
  });
  fireEvent.change(container.querySelector("input[type=file]")!, { target: { files: [file] } });

  expect(await screen.findByText("Finish local work")).toBeInTheDocument();
  expect(screen.getByText("Jira work").closest("label")?.querySelector("input")).toBeDisabled();
  await waitFor(() => expect(requests).toHaveLength(2));
  expect(screen.getByRole("button", { name: "Review import of 1" })).toBeEnabled();
  fireEvent.click(screen.getByRole("button", { name: "Review import of 1" }));
  expect(requests).toHaveLength(2);
  expect(screen.getByText(/Existing workers stay asleep and Legacy is not changed/)).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Import selected tasks" }));

  await waitFor(() => expect(requests).toHaveLength(3));
  expect(requests[2].body).toMatchObject({
    commit: { bundle_digest: "preview-digest", selected_source_ids: ["local-1"] },
  });
  expect(await screen.findByText("Imported safely")).toBeInTheDocument();
  expect(screen.getByText("1 task staged for review. No workers were started.")).toBeInTheDocument();
});

test("recovers an active migration receipt after reopening settings", async () => {
  vi.stubGlobal("fetch", vi.fn(async () => ok([{
    batch_id: "batch-recovered",
    bundle_digest: "digest",
    source_installation_id: "legacy-hive",
    source_snapshot_digest: "snapshot",
    imported_task_ids: ["task-1"],
    imported_source_ids: ["legacy-1"],
    imported_at: 123,
  }])));

  render(<LegacyMigrationSettings busy={false} operatorToken="token" />);

  expect(await screen.findByText("Imported safely")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Undo this untouched import" })).toBeEnabled();
});

function ok(body: unknown, status = 200) {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}

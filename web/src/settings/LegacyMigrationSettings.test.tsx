import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import LegacyMigrationSettings from "./LegacyMigrationSettings";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

test("discovers the normal local Legacy install without a file picker", async () => {
  const requests: string[] = [];
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL) => {
    const url = String(input);
    requests.push(url);
    if (url.endsWith("/tasks") || url.endsWith("/workers")) return ok([]);
    if (url.endsWith("/local")) return ok({
      format: "swarm-next-migration",
      version: 1,
      source: { installation_id: "legacy", exported_at: 1, snapshot_digest: "source" },
      tasks: [{ source_id: "local-1" }],
      workers: [],
    });
    if (url.endsWith("/tasks/preview")) return ok({
      bundle_digest: "digest", source_installation_id: "legacy", selectable: 1, skipped: 0, invalid: 0,
      records: [{ source_id: "local-1", title: "Continue local work", source_status: "assigned", target_state: "ready", priority: "normal", disposition: "ready", selectable: true, warnings: [] }],
    });
    if (url.endsWith("/workers/preview")) return ok({ bundle_digest: "digest", source_installation_id: "legacy", selectable: 0, skipped: 0, invalid: 0, records: [] });
    throw new Error(`unexpected ${url}`);
  }));

  render(<LegacyMigrationSettings busy={false} operatorToken="token" />);
  fireEvent.click(await screen.findByRole("button", { name: "Find my Legacy Hive" }));

  expect(await screen.findByText("Continue local work")).toBeInTheDocument();
  expect(requests.some((url) => url.endsWith("/local"))).toBe(true);
  expect(screen.getByRole("button", { name: "Review import of 1" })).toBeEnabled();
});

test("previews before importing and selects only server-approved records", async () => {
  const requests: Array<{ url: string; body: unknown }> = [];
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    requests.push({ url, body: init?.body ? JSON.parse(String(init.body)) : undefined });
    if (url.endsWith("/api/v1/migrations/legacy/tasks") || url.endsWith("/api/v1/migrations/legacy/workers")) return ok([]);
    if (url.endsWith("/workers/preview")) {
      return ok({
        bundle_digest: "preview-digest",
        source_installation_id: "legacy-hive",
        selectable: 0,
        skipped: 0,
        invalid: 0,
        records: [],
      });
    }
    if (url.endsWith("/tasks/preview")) {
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
    if (url.endsWith("/tasks/commit")) {
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
  fireEvent.click(screen.getByRole("button", { name: "Show 1 staying in Legacy" }));
  expect(screen.getByText("Jira work").closest("label")?.querySelector("input")).toBeDisabled();
  await waitFor(() => expect(requests).toHaveLength(4));
  expect(screen.getByRole("button", { name: "Review import of 1" })).toBeEnabled();
  fireEvent.click(screen.getByRole("button", { name: "Review import of 1" }));
  expect(requests).toHaveLength(4);
  expect(screen.getByText(/Existing workers stay asleep and Legacy is not changed/)).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Import selected tasks" }));

  await waitFor(() => expect(requests).toHaveLength(5));
  expect(requests[4].body).toMatchObject({
    commit: { bundle_digest: "preview-digest", selected_source_ids: ["local-1"] },
  });
  expect(await screen.findByText("Imported safely")).toBeInTheDocument();
  expect(screen.getByText("1 task staged for review. No workers were started.")).toBeInTheDocument();
});

test("keeps closed Legacy history out of the migration work list until requested", async () => {
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL) => {
    const url = String(input);
    if (url.endsWith("/tasks") || url.endsWith("/workers")) return ok([]);
    if (url.endsWith("/local")) return ok({
      format: "swarm-next-migration",
      version: 1,
      source: { installation_id: "legacy", exported_at: 1, snapshot_digest: "source" },
      tasks: [],
      workers: [],
    });
    if (url.endsWith("/workers/preview")) return ok({ bundle_digest: "digest", source_installation_id: "legacy", selectable: 0, skipped: 0, invalid: 0, records: [] });
    if (url.endsWith("/tasks/preview")) return ok({
      bundle_digest: "digest",
      source_installation_id: "legacy",
      selectable: 1,
      skipped: 3,
      invalid: 1,
      records: [
        { source_id: "open", title: "Carry this forward", source_status: "assigned", target_state: "ready", priority: "normal", disposition: "ready", selectable: true, warnings: [] },
        { source_id: "jira", title: "Canonical Jira work", source_status: "assigned", priority: "normal", disposition: "skipped_jira", selectable: false, warnings: [] },
        { source_id: "invalid", title: "Damaged Legacy task", source_status: "unknown", priority: "normal", disposition: "invalid", selectable: false, warnings: ["The source task is incomplete."] },
        { source_id: "closed-1", title: "Old completed work", source_status: "completed", priority: "normal", disposition: "skipped_closed", selectable: false, warnings: [] },
        { source_id: "closed-2", title: "Another closed task", source_status: "removed", priority: "normal", disposition: "skipped_closed", selectable: false, warnings: [] },
      ],
    });
    throw new Error(`unexpected ${url}`);
  }));

  render(<LegacyMigrationSettings busy={false} operatorToken="token" />);
  fireEvent.click(await screen.findByRole("button", { name: "Find my Legacy Hive" }));

  expect(await screen.findByText("Carry this forward")).toBeInTheDocument();
  expect(screen.queryByText("Old completed work")).not.toBeInTheDocument();
  expect(screen.queryByText("Canonical Jira work")).not.toBeInTheDocument();
  expect(screen.queryByText("Damaged Legacy task")).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Show 1 needing attention" }));
  expect(screen.getByText("Damaged Legacy task")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Show 1 staying in Legacy" }));
  expect(screen.getByText("Canonical Jira work")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Show 2 closed Legacy tasks" }));
  expect(screen.getByText("Old completed work")).toBeInTheDocument();
  expect(screen.getByText("Another closed task")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Hide closed history" })).toHaveAttribute("aria-pressed", "true");
});

test("recovers an active migration receipt after reopening settings", async () => {
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL) => {
    if (String(input).endsWith("/workers")) return ok([]);
    return ok([{
      batch_id: "batch-recovered",
      bundle_digest: "digest",
      source_installation_id: "legacy-hive",
      source_snapshot_digest: "snapshot",
      imported_task_ids: ["task-1"],
      imported_source_ids: ["legacy-1"],
      imported_at: 123,
    }]);
  }));

  render(<LegacyMigrationSettings busy={false} operatorToken="token" />);

  expect(await screen.findByText("Imported safely")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Undo this untouched import" })).toBeEnabled();
});

test("previews selected Legacy workers as sleeping before adding them", async () => {
  const requests: Array<{ url: string; body: unknown }> = [];
  let taskPreviewCount = 0;
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    requests.push({ url, body: init?.body ? JSON.parse(String(init.body)) : undefined });
    if (url.endsWith("/tasks") || url.endsWith("/workers")) return ok([]);
    if (url.endsWith("/tasks/preview")) {
      taskPreviewCount += 1;
      return ok(taskPreviewCount === 1
        ? { bundle_digest: "digest", source_installation_id: "legacy", selectable: 0, skipped: 0, invalid: 0, records: [] }
        : {
            bundle_digest: "digest",
            source_installation_id: "legacy",
            selectable: 1,
            skipped: 0,
            invalid: 0,
            records: [{
              source_id: "task-daisy",
              title: "Work now matched to Daisy",
              source_status: "assigned",
              target_state: "ready",
              priority: "normal",
              matched_worker_id: "next-daisy",
              matched_worker_name: "Daisy",
              disposition: "ready",
              selectable: true,
              warnings: [],
            }],
          });
    }
    if (url.endsWith("/workers/preview")) {
      return ok({
        bundle_digest: "digest",
        source_installation_id: "legacy",
        selectable: 1,
        skipped: 1,
        invalid: 0,
        records: [
          { source_id: "daisy", name: "Daisy", workspace: "/projects/daisy", provider: "claude_code", disposition: "ready", selectable: true, warnings: [] },
          { source_id: "root", name: "Project Root", workspace: "/projects", provider: "claude_code", disposition: "managed_by_next", selectable: false, warnings: ["Swarm Next Scout owns cross-repository work; Project Root is not duplicated."] },
        ],
      });
    }
    if (url.endsWith("/workers/commit")) {
      return ok({ batch_id: "workers-1", bundle_digest: "digest", source_installation_id: "legacy", imported_worker_ids: ["next-daisy"], imported_source_ids: ["daisy"], imported_at: 123 }, 201);
    }
    throw new Error(`unexpected ${url}`);
  }));
  const { container } = render(<LegacyMigrationSettings busy={false} operatorToken="token" />);
  const payload = JSON.stringify({ format: "swarm-next-migration", version: 1, source: { installation_id: "legacy", exported_at: 1, snapshot_digest: "source" }, tasks: [], workers: [{ source_id: "daisy" }] });
  const file = new File([payload], "legacy.json", { type: "application/json" });
  Object.defineProperty(file, "text", { value: async () => payload });
  fireEvent.change(container.querySelector("input[type=file]")!, { target: { files: [file] } });

  expect(await screen.findByText("Daisy")).toBeInTheDocument();
  expect(screen.getByText("Project Root").closest("label")?.querySelector("input")).toBeDisabled();
  fireEvent.click(screen.getByRole("button", { name: "Review 1 worker" }));
  expect(screen.getByText(/No Claude or Codex process starts/)).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Add selected workers" }));

  expect(await screen.findByText("Familiar crew added safely")).toBeInTheDocument();
  expect(await screen.findByText("Work now matched to Daisy")).toBeInTheDocument();
  expect(screen.getByText(/Task matches were refreshed/)).toBeInTheDocument();
  expect(taskPreviewCount).toBe(2);
  const commit = requests.find((request) => request.url.endsWith("/workers/commit"));
  expect(commit?.body).toMatchObject({ commit: { selected_source_ids: ["daisy"] } });
});

function ok(body: unknown, status = 200) {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}

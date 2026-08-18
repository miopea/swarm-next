import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import MemberControlRoom from "./MemberControlRoom";

afterEach(() => vi.unstubAllGlobals());

test("shows a Member her Keeper, convergence, projects, and local shared ownership", async () => {
  vi.stubGlobal("fetch", vi.fn((input: RequestInfo | URL) => {
    const url = String(input);
    if (url.endsWith("/members")) return Promise.resolve(ok([
      { hive_id: "hive-1", hive_name: "Meadow Hive", operator_id: "operator-1", operator_display_name: "Bea", role: "keeper", is_local: false },
      { hive_id: "hive-2", hive_name: "Clover Hive", operator_id: "operator-2", operator_display_name: "Cora", role: "member", is_local: true },
    ]));
    if (url.endsWith("/shared-work")) return Promise.resolve(ok([
      { id: "claim-1", apiary_id: "apiary-1", project_id: "10001", issue_id: "20001", issue_key: "WWD-101", home_node_id: "node-2", home_hive_id: "hive-2", home_operator_id: "operator-2", state: "confirmed", reserved_at: 1, reservation_expires_at: 2, confirmed_at: 2, released_at: null, project_key: "WWD", project_name: "Website Development", home_hive_name: "Clover Hive", home_operator_display_name: "Cora" },
      { id: "claim-2", apiary_id: "apiary-1", project_id: "10001", issue_id: "20002", issue_key: "WWD-102", home_node_id: "node-3", home_hive_id: "hive-3", home_operator_id: "operator-3", state: "confirmed", reserved_at: 1, reservation_expires_at: 2, confirmed_at: 2, released_at: null, project_key: "WWD", project_name: "Website Development", home_hive_name: "Fern Hive", home_operator_display_name: "Faye" },
    ]));
    if (url.endsWith("/handoffs")) return Promise.resolve(ok([]));
    if (url.endsWith("/handoff-targets")) return Promise.resolve(ok([{ node_id: "node-3", hive_id: "hive-3", hive_name: "Fern Hive", operator_id: "operator-3", operator_display_name: "Faye" }]));
    if (url.includes("/claims/claim-1/handoffs")) return Promise.resolve(ok({ id: "handoff-1", claim_id: "claim-1", state: "offered" }));
    if (url.endsWith("/tasks") && !url.endsWith("/steward/tasks")) return Promise.resolve(ok([{ id: "task-1", apiary_id: "apiary-1", source: "swarm", title: "Prepare shared brief", description: "", priority: "high", state: "ready", home_node_id: null, home_hive_id: null, revision: 1, created_at: 1, updated_at: 1 }]));
    if (url.endsWith("/sync-health")) return Promise.resolve(ok({ condition: "current", last_attempt_at: 100, last_success_at: 100, consecutive_failures: 0, next_attempt_at: null }));
    if (url.endsWith("/task-sync-status")) return Promise.resolve(ok({ cursor: 4, task_count: 1, last_applied_at: 100 }));
    if (url.endsWith("/task-outbox")) return Promise.resolve(ok([]));
    if (url.endsWith("/task-outbox-status")) return Promise.resolve(ok({ queued_count: 0, conflict_count: 0, rejected_count: 0, last_attempt_at: null }));
    if (url.endsWith("/steward/tasks")) return Promise.resolve(ok([]));
    if (url.endsWith("/steward/assists")) return Promise.resolve(ok({ incoming: [], outbox: [] }));
    if (url.endsWith("/local-executions")) return Promise.resolve(ok([]));
    if (url.endsWith("/my-stewardship")) return Promise.resolve(ok({
      schema_version: 1, protocol_version: 1, apiary_id: "apiary-1", member_node_id: "node-2", member_operator_id: "operator-2", generated_at: 100,
      stewardship: { id: "stewardship-1", apiary_id: "apiary-1", steward_operator_id: "operator-2", managed_hive_ids: ["hive-2"], capabilities: ["observe", "assist", "takeover"] },
      observations: [{ hive_id: "hive-2", ready_swarm_task_count: 2, active_swarm_task_count: 1, blocked_swarm_task_count: 1, review_swarm_task_count: 3, active_jira_claim_count: 4, last_shared_activity_at: 100 }],
    }));
    if (url.endsWith("/catalog-readiness")) return Promise.resolve(ok({
      acknowledgement: { apiary_id: "apiary-1", policy_revision: 1, promoted_project_catalog_digest: "digest", project_count: 1, snapshot_issued_at: 1, snapshot_expires_at: 2, acknowledged_at: 1 },
      jira_connection: "ready",
      projects: [{ project: { project_id: "10001", project_key: "WWD", project_name: "Website Development" }, binding_id: "binding-1", access_verified: true, workflow_mapped: true }],
      blockers: [],
    }));
    throw new Error(`Unexpected request: ${url}`);
  }));
  const onManage = vi.fn();
  const onOpenTasks = vi.fn();
  render(<MemberControlRoom identity={memberIdentity()} operatorToken="secret" onManage={onManage} onOpenTasks={onOpenTasks} />);

  expect(await screen.findByRole("heading", { name: "Grand Garden" })).toBeInTheDocument();
  expect(screen.getByLabelText("Member Apiary summary")).toHaveTextContent("KeeperMeadow HiveCatalogVerifiedProjects ready1/1My Jira claims1Keeper tasks1");
  expect(screen.getAllByText("Bea")).toHaveLength(2);
  expect(screen.getByRole("list", { name: "Member promoted Jira projects" })).toHaveTextContent("WWDWebsite DevelopmentReady");
  expect(screen.getByRole("list", { name: "Member shared work ownership" })).toHaveTextContent("WWD-101Website DevelopmentOwnedCora");
  expect(screen.getByRole("list", { name: "Member Keeper tasks" })).toHaveTextContent("Prepare shared briefready · high · revision 1UnassignedView only from Apiary");
  expect(screen.getByRole("list", { name: "Apiary Hive roster" })).toHaveTextContent("Meadow HiveBeaKeeperClover HiveCoraThis Hive");
  expect(screen.getByText("Keeper task cursor").parentElement).toHaveTextContent("4");
  const stewardship = screen.getByRole("heading", { name: "Trusted support for 1 Hive" }).closest("article");
  expect(stewardship).toHaveTextContent("Clover Hive");
  expect(stewardship).toHaveTextContent("Observe, Assist, Take over");
  const observations = screen.getByRole("list", { name: "Managed Hive shared-work status" });
  expect(observations).toHaveTextContent("Clover Hive");
  expect(observations).toHaveTextContent("Ready2Active1Blocked1Review3Jira owned4");
  expect(stewardship).toHaveTextContent("private workers and terminals stay local");
  expect(document.body).not.toHaveTextContent("WWD-102");
  expect(document.body).not.toHaveTextContent("node-2");
  expect(document.body).not.toHaveTextContent("secret");
  fireEvent.click(screen.getByRole("button", { name: "Offer to another Hive" }));
  fireEvent.change(screen.getByRole("combobox", { name: "Receiving Hive" }), { target: { value: "node-3" } });
  fireEvent.click(screen.getByRole("button", { name: "Send offer" }));
  await waitFor(() => expect(fetch).toHaveBeenCalledWith(expect.stringContaining("/claims/claim-1/handoffs"), expect.objectContaining({ method: "POST" })));
  fireEvent.click(screen.getByRole("button", { name: "Manage membership" }));
  expect(onManage).toHaveBeenCalledOnce();
  fireEvent.click(screen.getByRole("button", { name: "Manage in Tasks" }));
  expect(onOpenTasks).toHaveBeenCalledOnce();
});

test("keeps local work usable when part of the Member rollup is unavailable", async () => {
  vi.stubGlobal("fetch", vi.fn((input: RequestInfo | URL) => {
    const url = String(input);
    if (url.endsWith("/members")) return Promise.resolve(ok([]));
    if (url.endsWith("/shared-work")) return Promise.resolve(ok([]));
    if (url.endsWith("/handoffs")) return Promise.resolve(ok([]));
    if (url.endsWith("/handoff-targets")) return Promise.resolve(ok([]));
    if (url.endsWith("/tasks") && !url.endsWith("/steward/tasks")) return Promise.resolve(ok([]));
    if (url.endsWith("/sync-health")) return Promise.reject(new Error("offline"));
    if (url.endsWith("/task-sync-status")) return Promise.resolve(ok({ cursor: 0, task_count: 0, last_applied_at: null }));
    if (url.endsWith("/task-outbox")) return Promise.resolve(ok([]));
    if (url.endsWith("/task-outbox-status")) return Promise.resolve(ok({ queued_count: 0, conflict_count: 0, rejected_count: 0, last_attempt_at: null }));
    if (url.endsWith("/steward/tasks")) return Promise.resolve(ok([]));
    if (url.endsWith("/steward/assists")) return Promise.resolve(ok({ incoming: [], outbox: [] }));
    if (url.endsWith("/local-executions")) return Promise.resolve(ok([]));
    if (url.endsWith("/my-stewardship")) return Promise.resolve(ok(null));
    if (url.endsWith("/catalog-readiness")) return Promise.resolve(ok({ acknowledgement: null, jira_connection: "network_unavailable", projects: [], blockers: ["catalog_missing"] }));
    throw new Error(`Unexpected request: ${url}`);
  }));
  render(<MemberControlRoom identity={memberIdentity()} operatorToken="secret" onManage={() => undefined} onOpenTasks={() => undefined} />);
  expect(await screen.findByRole("alert")).toHaveTextContent("Local workers and owned work are unchanged");
  expect(screen.getByRole("list", { name: "Shared work blockers" })).toHaveTextContent("Keeper catalog has not arrived");
});

test("keeps Steward work routing in Tasks without exposing the target Hive's workers", async () => {
  const fetchMock = vi.fn((input: RequestInfo | URL) => {
    const url = String(input);
    if (url.endsWith("/members")) return Promise.resolve(ok([
      { hive_id: "hive-1", hive_name: "Meadow Hive", operator_id: "operator-1", operator_display_name: "Bea", role: "keeper", is_local: false },
      { hive_id: "hive-2", hive_name: "Clover Hive", operator_id: "operator-2", operator_display_name: "Cora", role: "member", is_local: true },
      { hive_id: "hive-3", hive_name: "Fern Hive", operator_id: "operator-3", operator_display_name: "Faye", role: "member", is_local: false },
    ]));
    if (url.endsWith("/shared-work") || url.endsWith("/handoffs") || url.endsWith("/handoff-targets") || url.endsWith("/task-outbox") || url.endsWith("/local-executions") || url.endsWith("/steward/tasks")) return Promise.resolve(ok([]));
    if (url.endsWith("/steward/assists")) return Promise.resolve(ok({ incoming: [], outbox: [] }));
    if (url.endsWith("/tasks")) return Promise.resolve(ok([]));
    if (url.endsWith("/sync-health")) return Promise.resolve(ok({ condition: "current", last_attempt_at: 1, last_success_at: 1, consecutive_failures: 0, next_attempt_at: null }));
    if (url.endsWith("/task-sync-status")) return Promise.resolve(ok({ cursor: 0, task_count: 0, last_applied_at: 1 }));
    if (url.endsWith("/task-outbox-status")) return Promise.resolve(ok({ queued_count: 0, conflict_count: 0, rejected_count: 0, last_attempt_at: null }));
    if (url.endsWith("/my-stewardship")) return Promise.resolve(ok({
      schema_version: 1, protocol_version: 1, apiary_id: "apiary-1", member_node_id: "node-2", member_operator_id: "operator-2", generated_at: 100,
      stewardship: { id: "stewardship-1", apiary_id: "apiary-1", steward_operator_id: "operator-2", managed_hive_ids: ["hive-3"], capabilities: ["observe", "assign"] },
    }));
    if (url.endsWith("/catalog-readiness")) return Promise.resolve(ok({ acknowledgement: null, jira_connection: "ready", projects: [], blockers: [] }));
    throw new Error(`Unexpected request: ${url}`);
  });
  vi.stubGlobal("fetch", fetchMock);
  const onOpenTasks = vi.fn();
  render(<MemberControlRoom identity={memberIdentity()} operatorToken="secret" onManage={() => undefined} onOpenTasks={onOpenTasks} />);

  expect(await screen.findByText("Route shared work from Tasks")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Open Tasks" }));
  expect(onOpenTasks).toHaveBeenCalledOnce();
  expect(fetchMock).not.toHaveBeenCalledWith(expect.stringContaining("/steward/tasks"), expect.objectContaining({ method: "POST" }));
  expect(document.body).not.toHaveTextContent("Fern worker");
});

test("offers and accepts Steward assistance without injecting a worker terminal", async () => {
  const fetchMock = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    if (url.endsWith("/steward/assists") && init?.method === "POST") {
      expect(JSON.parse(String(init.body))).toEqual({ target_hive_id: "hive-3", message: "I can help review the release decision." });
      return Promise.resolve(ok({ command: { id: "assist-command-1" }, state: "queued" }));
    }
    if (url.includes("/steward/assists/assist-1/response")) {
      expect(JSON.parse(String(init?.body))).toEqual({ decision: "accepted" });
      return Promise.resolve(ok({ command: { id: "response-1" }, state: "queued" }));
    }
    if (url.endsWith("/members")) return Promise.resolve(ok([
      { hive_id: "hive-1", hive_name: "Meadow Hive", operator_id: "operator-1", operator_display_name: "Bea", role: "keeper", is_local: false },
      { hive_id: "hive-2", hive_name: "Clover Hive", operator_id: "operator-2", operator_display_name: "Cora", role: "member", is_local: true },
      { hive_id: "hive-3", hive_name: "Fern Hive", operator_id: "operator-3", operator_display_name: "Faye", role: "member", is_local: false },
    ]));
    if (url.endsWith("/steward/assists")) return Promise.resolve(ok({ incoming: [{ id: "assist-1", apiary_id: "apiary-1", source_hive_id: "hive-3", target_hive_id: "hive-2", message: "I can help verify the incident recovery.", state: "pending", created_at: 1, resolved_at: null }], outbox: [] }));
    if (url.endsWith("/my-stewardship")) return Promise.resolve(ok({ schema_version: 1, protocol_version: 1, apiary_id: "apiary-1", member_node_id: "node-2", member_operator_id: "operator-2", generated_at: 100, stewardship: { id: "stewardship-1", apiary_id: "apiary-1", steward_operator_id: "operator-2", managed_hive_ids: ["hive-3"], capabilities: ["observe", "assist"] } }));
    if (url.endsWith("/sync-health")) return Promise.resolve(ok({ condition: "current", last_attempt_at: 1, last_success_at: 1, consecutive_failures: 0, next_attempt_at: null }));
    if (url.endsWith("/task-sync-status")) return Promise.resolve(ok({ cursor: 0, task_count: 0, last_applied_at: 1 }));
    if (url.endsWith("/task-outbox-status")) return Promise.resolve(ok({ queued_count: 0, conflict_count: 0, rejected_count: 0, last_attempt_at: null }));
    if (url.endsWith("/catalog-readiness")) return Promise.resolve(ok({ acknowledgement: null, jira_connection: "ready", projects: [], blockers: [] }));
    if (url.endsWith("/shared-work") || url.endsWith("/handoffs") || url.endsWith("/handoff-targets") || url.endsWith("/task-outbox") || url.endsWith("/local-executions") || url.endsWith("/steward/tasks") || url.endsWith("/tasks")) return Promise.resolve(ok([]));
    throw new Error(`Unexpected request: ${url}`);
  });
  vi.stubGlobal("fetch", fetchMock);
  render(<MemberControlRoom identity={memberIdentity()} operatorToken="secret" onManage={() => undefined} onOpenTasks={() => undefined} />);

  expect(await screen.findByRole("heading", { name: "A trusted Steward offered help" })).toBeInTheDocument();
  expect(screen.getByText("I can help verify the incident recovery.")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Accept help" }));
  await waitFor(() => expect(fetchMock).toHaveBeenCalledWith(expect.stringContaining("/assist-1/response"), expect.objectContaining({ method: "POST" })));
  const targetPicker = screen.getAllByRole("combobox", { name: "Assistance target Hive" }).at(-1)!;
  const assistComposer = screen.getAllByRole("textbox", { name: "Steward assistance message" }).at(-1)!;
  const offerButton = screen.getAllByRole("button", { name: "Offer help through Keeper" }).at(-1)!;
  fireEvent.change(targetPicker, { target: { value: "hive-3" } });
  fireEvent.change(assistComposer, { target: { value: "I can help review the release decision." } });
  fireEvent.click(offerButton);
  await waitFor(() => expect(fetchMock).toHaveBeenCalledWith(expect.stringMatching(/\/steward\/assists$/), expect.objectContaining({ method: "POST" })));
  expect(document.body).toHaveTextContent("never injected into a terminal");
});

function memberIdentity() { return { operator: { id: "operator-2", display_name: "Cora" }, hive: { id: "hive-2", name: "Clover Hive", operator_id: "operator-2", apiary_id: "apiary-1" }, apiary_context: { mode: "federated" as const, apiary: { id: "apiary-1", name: "Grand Garden", keeper_operator_id: "operator-1", shared_work_backend: "jira" as const }, local_role: "member" as const } }; }
function ok(payload: unknown) { return { ok: true, status: 200, json: async () => payload }; }

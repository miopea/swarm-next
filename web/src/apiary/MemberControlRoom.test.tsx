import { fireEvent, render, screen } from "@testing-library/react";
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
    if (url.endsWith("/tasks")) return Promise.resolve(ok([{ id: "task-1", apiary_id: "apiary-1", source: "swarm", title: "Prepare shared brief", description: "", priority: "high", state: "ready", home_node_id: null, home_hive_id: null, revision: 1, created_at: 1, updated_at: 1 }]));
    if (url.endsWith("/sync-health")) return Promise.resolve(ok({ condition: "current", last_attempt_at: 100, last_success_at: 100, consecutive_failures: 0, next_attempt_at: null }));
    if (url.endsWith("/task-sync-status")) return Promise.resolve(ok({ cursor: 4, task_count: 1, last_applied_at: 100 }));
    if (url.endsWith("/catalog-readiness")) return Promise.resolve(ok({
      acknowledgement: { apiary_id: "apiary-1", policy_revision: 1, promoted_project_catalog_digest: "digest", project_count: 1, snapshot_issued_at: 1, snapshot_expires_at: 2, acknowledged_at: 1 },
      jira_connection: "ready",
      projects: [{ project: { project_id: "10001", project_key: "WWD", project_name: "Website Development" }, binding_id: "binding-1", access_verified: true, workflow_mapped: true }],
      blockers: [],
    }));
    throw new Error(`Unexpected request: ${url}`);
  }));
  const onManage = vi.fn();
  render(<MemberControlRoom identity={memberIdentity()} operatorToken="secret" onManage={onManage} />);

  expect(await screen.findByRole("heading", { name: "Grand Garden" })).toBeInTheDocument();
  expect(screen.getByLabelText("Member Apiary summary")).toHaveTextContent("KeeperMeadow HiveCatalogVerifiedProjects ready1/1My Jira claims1Keeper tasks1");
  expect(screen.getByText("Bea")).toBeInTheDocument();
  expect(screen.getByRole("list", { name: "Member promoted Jira projects" })).toHaveTextContent("WWDWebsite DevelopmentReady");
  expect(screen.getByRole("list", { name: "Member shared work ownership" })).toHaveTextContent("WWD-101Website DevelopmentOwnedCora");
  expect(screen.getByRole("list", { name: "Member Keeper tasks" })).toHaveTextContent("Prepare shared briefready · highUnassignedRevision 1");
  expect(screen.getByText("Keeper task cursor").parentElement).toHaveTextContent("4");
  expect(document.body).not.toHaveTextContent("WWD-102");
  expect(document.body).not.toHaveTextContent("node-2");
  expect(document.body).not.toHaveTextContent("secret");
  fireEvent.click(screen.getByRole("button", { name: "Manage membership" }));
  expect(onManage).toHaveBeenCalledOnce();
});

test("keeps local work usable when part of the Member rollup is unavailable", async () => {
  vi.stubGlobal("fetch", vi.fn((input: RequestInfo | URL) => {
    const url = String(input);
    if (url.endsWith("/members")) return Promise.resolve(ok([]));
    if (url.endsWith("/shared-work")) return Promise.resolve(ok([]));
    if (url.endsWith("/tasks")) return Promise.resolve(ok([]));
    if (url.endsWith("/sync-health")) return Promise.reject(new Error("offline"));
    if (url.endsWith("/task-sync-status")) return Promise.resolve(ok({ cursor: 0, task_count: 0, last_applied_at: null }));
    if (url.endsWith("/catalog-readiness")) return Promise.resolve(ok({ acknowledgement: null, jira_connection: "network_unavailable", projects: [], blockers: ["catalog_missing"] }));
    throw new Error(`Unexpected request: ${url}`);
  }));
  render(<MemberControlRoom identity={memberIdentity()} operatorToken="secret" onManage={() => undefined} />);
  expect(await screen.findByRole("alert")).toHaveTextContent("Local workers and owned work are unchanged");
  expect(screen.getByRole("list", { name: "Shared work blockers" })).toHaveTextContent("Keeper catalog has not arrived");
});

function memberIdentity() { return { operator: { id: "operator-2", display_name: "Cora" }, hive: { id: "hive-2", name: "Clover Hive", operator_id: "operator-2", apiary_id: "apiary-1" }, apiary_context: { mode: "federated" as const, apiary: { id: "apiary-1", name: "Grand Garden", keeper_operator_id: "operator-1", shared_work_backend: "jira" as const }, local_role: "member" as const } }; }
function ok(payload: unknown) { return { ok: true, status: 200, json: async () => payload }; }

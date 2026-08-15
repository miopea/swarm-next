import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import KeeperControlRoom from "./KeeperControlRoom";

afterEach(() => vi.unstubAllGlobals());

test("shows a low-noise Keeper rollup from public Apiary records", async () => {
  vi.stubGlobal("fetch", vi.fn((input: RequestInfo | URL) => {
    const url = String(input);
    if (url.endsWith("/members")) return Promise.resolve(ok([{ hive_id: "hive-1", hive_name: "Meadow Hive", operator_id: "operator-1", operator_display_name: "Bea", role: "keeper", is_local: true }, { hive_id: "hive-2", hive_name: "Clover Hive", operator_id: "operator-2", operator_display_name: "Cora", role: "member", is_local: false }]));
    if (url.endsWith("/jira-projects")) return Promise.resolve(ok([{ apiary_id: "apiary-1", project_id: "10001", project_key: "WWD", project_name: "Website Development", promoted_by_operator_id: "operator-1", promoted_at: 1 }]));
    if (url.endsWith("/shared-work")) return Promise.resolve(ok([{ id: "claim-1", apiary_id: "apiary-1", project_id: "10001", issue_id: "20001", issue_key: "WWD-101", home_node_id: "node-2", home_hive_id: "hive-2", home_operator_id: "operator-2", state: "confirmed", reserved_at: 1, reservation_expires_at: 2, confirmed_at: 2, released_at: null, project_key: "WWD", project_name: "Website Development", home_hive_name: "Clover Hive", home_operator_display_name: "Cora" }]));
    if (url.endsWith("/tasks")) return Promise.resolve(ok([{ id: "task-1", apiary_id: "apiary-1", source: "swarm", title: "Coordinate release", description: "", priority: "normal", state: "ready", home_node_id: null, home_hive_id: null, revision: 1, created_at: 1, updated_at: 1 }]));
    if (url.endsWith("/stewardships")) return Promise.resolve(ok([{ id: "steward-1", apiary_id: "apiary-1", steward_operator_id: "operator-2", managed_hive_ids: ["hive-2"], capabilities: ["observe", "assist"] }]));
    throw new Error(`Unexpected request: ${url}`);
  }));
  const onManage = vi.fn();
  render(<KeeperControlRoom identity={keeperIdentity()} operatorToken="secret" onManage={onManage} />);
  expect(await screen.findByRole("heading", { name: "Grand Garden" })).toBeInTheDocument();
  expect(await screen.findByLabelText("Apiary summary")).toHaveTextContent("Registered Hives2Promoted Jira projects1Active Jira claims1Swarm tasks1Steward scopes1");
  expect(screen.getByRole("list", { name: "Keeper Apiary Hives" })).toHaveTextContent("Meadow HiveBeaKeeper · This HiveClover HiveCoraHive");
  expect(screen.getByRole("list", { name: "Keeper shared work ownership" })).toHaveTextContent("WWD-101WWD · OwnedClover HiveCora");
  expect(screen.getByRole("list", { name: "Keeper Swarm tasks" })).toHaveTextContent("Coordinate releaseSwarm · readyUnassignedRevision 1");
  expect(screen.getByRole("list", { name: "Keeper promoted Jira projects" })).toHaveTextContent("WWDWebsite Development");
  expect(screen.getByRole("list", { name: "Keeper Steward scopes" })).toHaveTextContent("CoraClover HiveClover Hive");
  expect(document.body).not.toHaveTextContent("node-2");
  expect(document.body).not.toHaveTextContent("secret");
  fireEvent.click(screen.getByRole("button", { name: "Manage Apiary" }));
  expect(onManage).toHaveBeenCalledOnce();
});

function keeperIdentity() { return { operator: { id: "operator-1", display_name: "Bea" }, hive: { id: "hive-1", name: "Meadow Hive", operator_id: "operator-1", apiary_id: "apiary-1" }, apiary_context: { mode: "federated" as const, apiary: { id: "apiary-1", name: "Grand Garden", keeper_operator_id: "operator-1", shared_work_backend: "jira" as const }, local_role: "keeper" as const } }; }
function ok(payload: unknown) { return { ok: true, status: 200, json: async () => payload }; }

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import KeeperControlRoom from "./KeeperControlRoom";

afterEach(() => { cleanup(); vi.unstubAllGlobals(); });

test("shows a low-noise Keeper rollup from public Apiary records", async () => {
  vi.stubGlobal("fetch", vi.fn((input: RequestInfo | URL) => {
    const url = String(input);
    if (url.endsWith("/members")) return Promise.resolve(ok([{ hive_id: "hive-1", hive_name: "Meadow Hive", operator_id: "operator-1", operator_display_name: "Bea", role: "keeper", is_local: true }, { hive_id: "hive-2", hive_name: "Clover Hive", operator_id: "operator-2", operator_display_name: "Cora", role: "member", is_local: false }, { hive_id: "hive-3", hive_name: "Fern Hive", operator_id: "operator-3", operator_display_name: "Faye", role: "member", is_local: false }]));
    if (url.endsWith("/jira-projects")) return Promise.resolve(ok([{ apiary_id: "apiary-1", project_id: "10001", project_key: "WWD", project_name: "Website Development", promoted_by_operator_id: "operator-1", promoted_at: 1 }]));
    if (url.endsWith("/shared-work")) return Promise.resolve(ok([{ id: "claim-1", apiary_id: "apiary-1", project_id: "10001", issue_id: "20001", issue_key: "WWD-101", home_node_id: "node-2", home_hive_id: "hive-2", home_operator_id: "operator-2", state: "confirmed", reserved_at: 1, reservation_expires_at: 2, confirmed_at: 2, released_at: null, project_key: "WWD", project_name: "Website Development", home_hive_name: "Clover Hive", home_operator_display_name: "Cora" }]));
    if (url.endsWith("/tasks")) return Promise.resolve(ok([{ id: "task-1", apiary_id: "apiary-1", source: "swarm", title: "Coordinate release", description: "", priority: "normal", state: "ready", home_node_id: null, home_hive_id: null, revision: 1, created_at: 1, updated_at: 1 }]));
    if (url.endsWith("/stewardships")) return Promise.resolve(ok([{ id: "steward-1", apiary_id: "apiary-1", steward_operator_id: "operator-2", managed_hive_ids: ["hive-2"], capabilities: ["observe", "assist"] }]));
    if (url.endsWith("/handoffs")) return Promise.resolve(ok([{ id: "handoff-1", apiary_id: "apiary-1", claim_id: "claim-1", project_id: "10001", issue_id: "20001", issue_key: "WWD-101", source_node_id: "node-2", source_hive_id: "hive-2", source_operator_id: "operator-2", target_node_id: "node-3", target_hive_id: "hive-3", target_operator_id: "operator-3", state: "offered", reason: "Repository expertise", offered_at: 3, accepted_at: null, completed_at: null, closed_at: null }]));
    throw new Error(`Unexpected request: ${url}`);
  }));
  const onManage = vi.fn();
  render(<KeeperControlRoom identity={keeperIdentity()} operatorToken="secret" onManage={onManage} />);
  expect(await screen.findByRole("heading", { name: "Grand Garden" })).toBeInTheDocument();
  expect(await screen.findByLabelText("Apiary summary")).toHaveTextContent("Registered Hives3Promoted Jira projects1Active Jira claims1Work handoffs1Swarm tasks1Steward scopes1");
  expect(screen.getByRole("list", { name: "Keeper Apiary Hives" })).toHaveTextContent("Meadow HiveBeaKeeper · This HiveClover HiveCoraHiveFern HiveFayeHive");
  expect(screen.getByRole("list", { name: "Keeper shared work ownership" })).toHaveTextContent("WWD-101WWD · OwnedClover HiveCora");
  expect(screen.getByRole("list", { name: "Keeper Swarm tasks" })).toHaveTextContent("Coordinate releaseSwarm · readyUnassignedAvailable to claim · revision 1");
  expect(screen.getByRole("list", { name: "Keeper promoted Jira projects" })).toHaveTextContent("WWDWebsite Development");
  expect(screen.getByRole("list", { name: "Keeper Steward scopes" })).toHaveTextContent("CoraClover HiveClover Hive");
  expect(screen.getByRole("list", { name: "Keeper active Jira handoffs" })).toHaveTextContent("WWD-101Awaiting acceptanceClover Hive → Fern HiveRepository expertise");
  expect(document.body).not.toHaveTextContent("node-2");
  expect(document.body).not.toHaveTextContent("secret");
  fireEvent.click(screen.getByRole("button", { name: "Manage Apiary" }));
  expect(onManage).toHaveBeenCalledOnce();
});

test("creates Keeper-canonical shared work without choosing a private worker", async () => {
  const requests: Array<{ url: string; init?: RequestInit }> = [];
  let tasks: unknown[] = [];
  vi.stubGlobal("fetch", vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    requests.push({ url, init });
    if (url.endsWith("/members") || url.endsWith("/jira-projects") || url.endsWith("/shared-work") || url.endsWith("/stewardships") || url.endsWith("/handoffs")) return Promise.resolve(ok([]));
    if (url.endsWith("/tasks") && init?.method === "POST") {
      const created = { id: "task-2", apiary_id: "apiary-1", source: "swarm", title: "Coordinate the release", description: "Keep both Hives aligned.", priority: "high", state: "ready", home_node_id: null, home_hive_id: null, revision: 1, created_at: 2, updated_at: 2 };
      tasks = [created];
      return Promise.resolve({ ...ok(created), status: 201 });
    }
    if (url.endsWith("/tasks")) return Promise.resolve(ok(tasks));
    throw new Error(`Unexpected request: ${url}`);
  }));
  render(<KeeperControlRoom identity={keeperIdentity()} operatorToken="secret" onManage={vi.fn()} />);
  await screen.findByRole("heading", { name: "Grand Garden" });
  fireEvent.click(screen.getByRole("button", { name: "Create shared task" }));
  expect(screen.queryByLabelText(/worker/i)).not.toBeInTheDocument();
  fireEvent.change(screen.getByLabelText("Outcome"), { target: { value: "  Coordinate the release  " } });
  fireEvent.change(screen.getByLabelText(/Context/), { target: { value: "  Keep both Hives aligned.  " } });
  fireEvent.change(screen.getByLabelText("Priority"), { target: { value: "high" } });
  fireEvent.click(screen.getByRole("button", { name: "Create for Apiary" }));
  await waitFor(() => expect(screen.getByRole("list", { name: "Keeper Swarm tasks" })).toHaveTextContent("Coordinate the releaseSwarm · readyUnassignedAvailable to claim · revision 1"));
  const request = requests.find(({ url, init }) => url.endsWith("/tasks") && init?.method === "POST");
  expect(JSON.parse(String(request?.init?.body))).toEqual({ title: "Coordinate the release", description: "Keep both Hives aligned.", priority: "high" });
  expect(screen.queryByRole("form", { name: "Create shared Apiary task" })).not.toBeInTheDocument();
});

test("routes Keeper work to a public Member Hive without selecting her private worker", async () => {
  const requests: Array<{ url: string; init?: RequestInit }> = [];
  let tasks: unknown[] = [];
  vi.stubGlobal("fetch", vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    requests.push({ url, init });
    if (url.endsWith("/members")) return Promise.resolve(ok([
      { hive_id: "hive-1", hive_name: "Meadow Hive", operator_id: "operator-1", operator_display_name: "Bea", role: "keeper", is_local: true },
      { hive_id: "hive-2", hive_name: "Clover Hive", operator_id: "operator-2", operator_display_name: "Cora", role: "member", is_local: false },
    ]));
    if (url.endsWith("/jira-projects") || url.endsWith("/shared-work") || url.endsWith("/stewardships") || url.endsWith("/handoffs")) return Promise.resolve(ok([]));
    if (url.endsWith("/tasks") && init?.method === "POST") {
      const created = { id: "task-routed", apiary_id: "apiary-1", source: "swarm", title: "Prepare Clover release", description: "", priority: "normal", state: "ready", home_node_id: "private-node", home_hive_id: "hive-2", revision: 1, created_at: 2, updated_at: 2 };
      tasks = [created];
      return Promise.resolve({ ...ok(created), status: 201 });
    }
    if (url.endsWith("/tasks")) return Promise.resolve(ok(tasks));
    throw new Error(`Unexpected request: ${url}`);
  }));
  render(<KeeperControlRoom identity={keeperIdentity()} operatorToken="secret" onManage={vi.fn()} />);
  await screen.findByRole("heading", { name: "Grand Garden" });
  fireEvent.click(screen.getByRole("button", { name: "Create shared task" }));
  fireEvent.change(screen.getByLabelText("Outcome"), { target: { value: "Prepare Clover release" } });
  fireEvent.change(screen.getByLabelText(/Route to Hive/), { target: { value: "hive-2" } });
  expect(screen.getByText("The selected Hive owns this work. Her Queen chooses the private worker and repository.")).toBeInTheDocument();
  expect(screen.queryByLabelText(/worker/i)).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Route to Hive" }));
  await waitFor(() => expect(screen.getByRole("list", { name: "Keeper Swarm tasks" })).toHaveTextContent("Prepare Clover releaseSwarm · readyClover HiveRouted by Keeper · revision 1"));
  const request = requests.find(({ url, init }) => url.endsWith("/tasks") && init?.method === "POST");
  expect(JSON.parse(String(request?.init?.body))).toEqual({ title: "Prepare Clover release", description: "", priority: "normal", home_hive_id: "hive-2" });
  expect(document.body).not.toHaveTextContent("private-node");
});

function keeperIdentity() { return { operator: { id: "operator-1", display_name: "Bea" }, hive: { id: "hive-1", name: "Meadow Hive", operator_id: "operator-1", apiary_id: "apiary-1" }, apiary_context: { mode: "federated" as const, apiary: { id: "apiary-1", name: "Grand Garden", keeper_operator_id: "operator-1", shared_work_backend: "jira" as const }, local_role: "keeper" as const } }; }
function ok(payload: unknown) { return { ok: true, status: 200, json: async () => payload }; }

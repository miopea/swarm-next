import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import type { HiveIdentity } from "../api";
import ApiarySettings from "./ApiarySettings";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

test("founds only a reviewed Jira-backed Apiary and refreshes Hive identity", async () => {
  const onHiveIdentityChange = vi.fn();
  const federated = keeperIdentity();
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    if (url === "/api/v1/apiary") {
      expect(init?.method).toBe("POST");
      expect(JSON.parse(String(init?.body))).toEqual({
        name: "Wildflower Garden",
        shared_work_backend: "jira",
      });
      return ok(federated.apiary_context, 201);
    }
    if (url === "/api/v1/hive") return ok(federated);
    throw new Error(`unexpected request ${url}`);
  }));

  render(<ApiarySettings busy={false} hiveIdentity={personalIdentity()} operatorToken="secret" onHiveIdentityChange={onHiveIdentityChange} />);

  expect(screen.getByText("Jira-backed").parentElement).toHaveTextContent("Available now");
  expect(screen.getByText("Native Swarm").parentElement).toHaveAttribute("aria-disabled", "true");
  const review = screen.getByRole("button", { name: "Review Apiary setup" });
  expect(review).toBeDisabled();
  fireEvent.change(screen.getByLabelText("Apiary name"), { target: { value: "  Wildflower Garden  " } });
  expect(review).toBeEnabled();
  fireEvent.click(review);
  expect(screen.getByRole("group", { name: "Confirm Apiary setup" })).toHaveTextContent("backend cannot be converted later");
  fireEvent.click(screen.getByRole("button", { name: "Found Jira-backed Apiary" }));

  await vi.waitFor(() => expect(onHiveIdentityChange).toHaveBeenCalledWith(federated));
});

test("downloads a short-lived signed Hive card without changing membership", async () => {
  const createObjectUrl = vi.fn(() => "blob:connection-card");
  const revokeObjectUrl = vi.fn();
  vi.stubGlobal("URL", { ...URL, createObjectURL: createObjectUrl, revokeObjectURL: revokeObjectUrl });
  const click = vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => undefined);
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL) => {
    if (String(input) === "/api/v1/apiary/connection-card") {
      return ok({
        payload: {
          schema_version: 1, protocol_version: 1, node_id: "node-1", hive_id: "hive-1",
          hive_name: "Meadow Hive", operator_id: "operator-1", operator_display_name: "Bea",
          public_key: "public", issued_at: 10, expires_at: 86_410,
        },
        signature: "signed",
      });
    }
    throw new Error(`unexpected request ${String(input)}`);
  }));

  render(<ApiarySettings busy={false} hiveIdentity={personalIdentity()} operatorToken="secret" onHiveIdentityChange={vi.fn()} />);
  fireEvent.click(screen.getByRole("button", { name: "Download connection card" }));

  expect(await screen.findByRole("status")).toHaveTextContent("expires in 24 hours and grants no access");
  expect(createObjectUrl).toHaveBeenCalledWith(expect.any(Blob));
  expect(click).toHaveBeenCalledOnce();
  expect(revokeObjectUrl).toHaveBeenCalledWith("blob:connection-card");
  expect(screen.getByText(/Your personal Hive remains fully independent/)).toBeVisible();
});

test("shows every collapse blocker and cannot bypass the disabled action", async () => {
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL) => {
    if (String(input) === "/api/v1/apiary/collapse-readiness") {
      return ok({
        active_hive_count: 2,
        pending_invitation_count: 1,
        active_stewardship_count: 1,
        open_cross_hive_work_count: 2,
        departed_node_count: 1,
      });
    }
    throw new Error(`unexpected request ${String(input)}`);
  }));

  render(<ApiarySettings busy={false} hiveIdentity={keeperIdentity()} operatorToken="secret" onHiveIdentityChange={vi.fn()} />);

  expect(await screen.findByText(/Clear before returning/)).toHaveTextContent("2 active Hives, 1 invitation, 1 Stewardship, 2 cross-Hive work items, 1 departed node");
  expect(screen.getByRole("button", { name: "Review return to personal Hive" })).toBeDisabled();
});

test("collapses a ready sole-Keeper Apiary only after inline confirmation", async () => {
  const personal = personalIdentity();
  const onHiveIdentityChange = vi.fn();
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    if (url === "/api/v1/apiary/collapse-readiness") {
      return ok({ active_hive_count: 1, pending_invitation_count: 0, active_stewardship_count: 0, open_cross_hive_work_count: 0, departed_node_count: 0 });
    }
    if (url === "/api/v1/apiary/collapse") {
      expect(init?.method).toBe("POST");
      return ok({ mode: "personal" });
    }
    if (url === "/api/v1/hive") return ok(personal);
    throw new Error(`unexpected request ${url}`);
  }));

  render(<ApiarySettings busy={false} hiveIdentity={keeperIdentity()} operatorToken="secret" onHiveIdentityChange={onHiveIdentityChange} />);
  const review = await screen.findByRole("button", { name: "Review return to personal Hive" });
  expect(review).toBeEnabled();
  fireEvent.click(review);
  expect(screen.getByRole("group", { name: "Confirm return to personal Hive" })).toHaveTextContent("lifecycle audit remain preserved");
  fireEvent.click(screen.getByRole("button", { name: "Return to personal Hive" }));

  await vi.waitFor(() => expect(onHiveIdentityChange).toHaveBeenCalledWith(personal));
});

test("promotes only a ready Hive Jira project and then shows it as Apiary owned", async () => {
  let promoted = false;
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    if (url === "/api/v1/apiary/collapse-readiness") {
      return ok({ active_hive_count: 1, pending_invitation_count: 0, active_stewardship_count: 0, open_cross_hive_work_count: 0, departed_node_count: 0 });
    }
    if (url === "/api/v1/apiary/jira-projects" && !init?.method) {
      return ok(promoted ? [{
        apiary_id: "apiary-1", project_id: "10001", project_key: "WEB",
        project_name: "Website Services", promoted_by_operator_id: "operator-1", promoted_at: 20,
      }] : []);
    }
    if (url === "/api/v1/integrations/jira/bindings") {
      return ok([{
        id: "binding-1", project_id: "10001", project_key: "WEB", project_name: "Website Services",
        scope: promoted ? "apiary" : "hive", hive_id: "hive-1", apiary_id: promoted ? "apiary-1" : null,
        access_verified: true, workflow_mapped: true, auto_sync_assigned: true,
      }]);
    }
    if (url === "/api/v1/apiary/jira-projects/binding-1/promotion") {
      expect(init?.method).toBe("POST");
      promoted = true;
      return ok({
        apiary_id: "apiary-1", project_id: "10001", project_key: "WEB",
        project_name: "Website Services", promoted_by_operator_id: "operator-1", promoted_at: 20,
      }, 201);
    }
    throw new Error(`unexpected request ${url}`);
  }));

  render(<ApiarySettings busy={false} hiveIdentity={keeperIdentity()} operatorToken="secret" onHiveIdentityChange={vi.fn()} />);

  expect(await screen.findByText("Ready to promote")).toBeVisible();
  fireEvent.click(screen.getByRole("button", { name: "Promote project" }));
  expect(await screen.findByRole("list", { name: "Promoted Jira projects" })).toHaveTextContent("WEBWebsite ServicesApiary catalog");
  expect(screen.getByRole("status")).toHaveTextContent("WEB is now in the Apiary project catalog");
});

function personalIdentity(): HiveIdentity {
  return {
    operator: { id: "operator-1", display_name: "Bea" },
    hive: { id: "hive-1", name: "Meadow Hive", operator_id: "operator-1", apiary_id: null },
    apiary_context: { mode: "personal" },
  };
}

function keeperIdentity(): HiveIdentity {
  return {
    operator: { id: "operator-1", display_name: "Bea" },
    hive: { id: "hive-1", name: "Meadow Hive", operator_id: "operator-1", apiary_id: "apiary-1" },
    apiary_context: {
      mode: "federated",
      apiary: { id: "apiary-1", name: "Wildflower Garden", keeper_operator_id: "operator-1", shared_work_backend: "jira" },
      local_role: "keeper",
    },
  };
}

function ok(body: unknown, status = 200) {
  return new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json" } });
}

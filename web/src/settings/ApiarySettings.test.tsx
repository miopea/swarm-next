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
    const url = String(input);
    if (url === "/api/v1/apiary/collapse-readiness") {
      return ok({
        active_hive_count: 2,
        pending_invitation_count: 1,
        active_stewardship_count: 1,
        open_cross_hive_work_count: 2,
        departed_node_count: 1,
      });
    }
    if (url === "/api/v1/apiary/hive-candidates") return ok([]);
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
    if (url === "/api/v1/apiary/hive-candidates") return ok([]);
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
    if (url === "/api/v1/apiary/hive-candidates") return ok([]);
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

test("pins an imported Hive identity without implying membership or access", async () => {
  const card = {
    payload: {
      schema_version: 1, protocol_version: 1, node_id: "node-2", hive_id: "hive-2",
      hive_name: "Clover Hive", operator_id: "operator-2", operator_display_name: "Cora",
      public_key: "public", issued_at: 10, expires_at: 86_410,
    },
    signature: "signed",
  };
  const candidate = {
    apiary_id: "apiary-1", node_id: "node-2", hive_id: "hive-2", hive_name: "Clover Hive",
    operator_id: "operator-2", operator_display_name: "Cora", public_key: "public",
    card_issued_at: 10, card_expires_at: 86_410,
    pinned_by_operator_id: "operator-1", pinned_at: 20, last_verified_at: 20,
  };
  let pinned = false;
  const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    if (url === "/api/v1/apiary/collapse-readiness") {
      return ok({ active_hive_count: 1, pending_invitation_count: 0, active_stewardship_count: 0, open_cross_hive_work_count: 0, departed_node_count: 0 });
    }
    if (url === "/api/v1/apiary/jira-projects") return ok([]);
    if (url === "/api/v1/integrations/jira/bindings") return ok([]);
    if (url === "/api/v1/apiary/hive-candidates" && init?.method === "POST") {
      expect(JSON.parse(String(init.body))).toEqual(card);
      pinned = true;
      return ok(candidate, 201);
    }
    if (url === "/api/v1/apiary/hive-candidates") return ok(pinned ? [candidate] : []);
    throw new Error(`unexpected request ${url}`);
  });
  vi.stubGlobal("fetch", fetchMock);

  render(<ApiarySettings busy={false} hiveIdentity={keeperIdentity()} operatorToken="secret" onHiveIdentityChange={vi.fn()} />);
  const file = { size: 512, text: vi.fn(async () => JSON.stringify(card)) } as unknown as File;
  fireEvent.change(screen.getByLabelText("Choose Hive connection card"), { target: { files: [file] } });

  expect(await screen.findByRole("status")).toHaveTextContent("verified and pinned. No membership or access was granted");
  expect(screen.getByRole("list", { name: "Pinned Hive identities" })).toHaveTextContent("Clover HiveCoraIdentity pinned");
  expect(fetchMock.mock.calls.some(([url]) => String(url).includes("/invitations"))).toBe(false);
});

test("downloads one invitation secret for a pinned Hive and then shows it pending", async () => {
  const createObjectUrl = vi.fn(() => "blob:invitation");
  const revokeObjectUrl = vi.fn();
  vi.stubGlobal("URL", { ...URL, createObjectURL: createObjectUrl, revokeObjectURL: revokeObjectUrl });
  const click = vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => undefined);
  let invited = false;
  const candidate = {
    apiary_id: "apiary-1", node_id: "node-2", hive_id: "hive-2", hive_name: "Clover Hive",
    operator_id: "operator-2", operator_display_name: "Cora", public_key: "public",
    card_issued_at: 10, card_expires_at: 86_410,
    pinned_by_operator_id: "operator-1", pinned_at: 20, last_verified_at: 20,
    invitation_pending: invited,
  };
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    if (url === "/api/v1/apiary/collapse-readiness") {
      return ok({ active_hive_count: 1, pending_invitation_count: invited ? 1 : 0, active_stewardship_count: 0, open_cross_hive_work_count: 0, departed_node_count: 0 });
    }
    if (url === "/api/v1/apiary/jira-projects") return ok([]);
    if (url === "/api/v1/integrations/jira/bindings") return ok([]);
    if (url === "/api/v1/apiary/hive-candidates/hive-2/invitation") {
      expect(init?.method).toBe("POST");
      invited = true;
      return ok({
        keeper_connection_card: { payload: { hive_name: "Meadow Hive" }, signature: "keeper" },
        invitation: { payload: { invitation_id: "invite-1", invited_hive_id: "hive-2" }, signature: "signed" },
        promoted_projects: [],
        one_time_secret: "shown-once",
      }, 201);
    }
    if (url === "/api/v1/apiary/hive-candidates") return ok([{ ...candidate, invitation_pending: invited }]);
    throw new Error(`unexpected request ${url}`);
  }));

  render(<ApiarySettings busy={false} hiveIdentity={keeperIdentity()} operatorToken="secret" onHiveIdentityChange={vi.fn()} />);
  fireEvent.click(await screen.findByRole("button", { name: "Create invitation" }));

  expect(await screen.findByRole("status")).toHaveTextContent("only copy of its one-time secret");
  expect(screen.getByRole("button", { name: "Invitation created" })).toBeDisabled();
  expect(createObjectUrl).toHaveBeenCalledWith(expect.any(Blob));
  expect(click).toHaveBeenCalledOnce();
  expect(revokeObjectUrl).toHaveBeenCalledWith("blob:invitation");
});

test("reviews an invitation before explicitly pinning its exact Keeper", async () => {
  const bundle = {
    keeper_connection_card: {
      payload: {
        schema_version: 1, protocol_version: 1, node_id: "keeper-node", hive_id: "keeper-hive",
        hive_name: "Rose Hive", operator_id: "keeper-operator", operator_display_name: "Rosa",
        public_key: "public", issued_at: 10, expires_at: 86_410,
      },
      signature: "keeper-card-signature",
    },
    invitation: {
      payload: {
        schema_version: 1, protocol_version: 1, invitation_id: "invite-1", apiary_id: "apiary-1",
        apiary_name: "Wildflower Garden", shared_work_backend: "jira" as const,
        required_policy_revision: 3, promoted_project_catalog_digest: "digest",
        keeper_node_id: "keeper-node", keeper_hive_id: "keeper-hive",
        keeper_operator_id: "keeper-operator", invited_node_id: "node-1", invited_hive_id: "hive-1",
        invited_operator_id: "operator-1", keeper_endpoint: "https://keeper.example.test/swarm",
        issued_at: 10, expires_at: 86_410, nonce: "nonce",
      },
      signature: "invitation-signature",
    },
    promoted_projects: [
      { project_id: "10000", project_key: "WWD", project_name: "Website Development" },
    ],
    one_time_secret: "private-secret",
  };
  const imported = {
    invitation_id: "invite-1", apiary_id: "apiary-1", apiary_name: "Wildflower Garden",
    shared_work_backend: "jira", required_policy_revision: 3,
    promoted_project_catalog_digest: "digest", keeper_node_id: "keeper-node",
    promoted_projects: [{ project_id: "10000", project_key: "WWD", project_name: "Website Development" }],
    keeper_hive_id: "keeper-hive", keeper_hive_name: "Rose Hive",
    keeper_operator_id: "keeper-operator", keeper_operator_display_name: "Rosa",
    keeper_endpoint: "https://keeper.example.test/swarm", state: "keeper_pinned",
    imported_at: 20, expires_at: 86_410,
  };
  let saved = false;
  const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    if (url === "/api/v1/apiary/join-invitations" && init?.method === "POST") {
      expect(JSON.parse(String(init.body))).toEqual(bundle);
      saved = true;
      return ok(imported, 201);
    }
    if (url === "/api/v1/apiary/join-invitations") return ok(saved ? [imported] : []);
    throw new Error(`unexpected request ${url}`);
  });
  vi.stubGlobal("fetch", fetchMock);

  render(<ApiarySettings busy={false} hiveIdentity={personalIdentity()} operatorToken="secret" onHiveIdentityChange={vi.fn()} />);
  const file = { size: 1024, text: vi.fn(async () => JSON.stringify(bundle)) } as unknown as File;
  fireEvent.change(screen.getByLabelText("Choose Apiary invitation"), { target: { files: [file] } });

  const review = await screen.findByRole("group", { name: "Review Apiary invitation" });
  expect(review).toHaveTextContent("Wildflower Garden");
  expect(review).toHaveTextContent("Rose Hive");
  expect(review).toHaveTextContent("Rosa");
  expect(review).toHaveTextContent("Policy revision3");
  expect(review).toHaveTextContent("Shared Jira projects1");
  expect(screen.getByRole("list", { name: "Promoted Jira projects" })).toHaveTextContent("WWDWebsite Development");
  expect(saved).toBe(false);
  fireEvent.click(screen.getByRole("button", { name: "Trust Keeper and save invitation" }));

  expect(await screen.findByRole("status")).toHaveTextContent("You have not joined or accepted its policy yet");
  expect(screen.getByRole("list", { name: "Saved Apiary invitations" })).toHaveTextContent("Keeper pinned · policy not accepted");
  expect(saved).toBe(true);
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

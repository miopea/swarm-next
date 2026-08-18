import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import type { HiveIdentity } from "../api";
import ApiarySettings from "./ApiarySettings";
import { clearStagedApiaryHandoff, createApiaryHandoffLink, stageApiaryHandoff } from "./apiaryHandoff";

test("connects outward to a Keeper without requiring an inbound member URL", async () => {
  const capability = {
    link_id: "link-1",
    keeper_endpoint: "https://keeper.example.test",
    secret: "private-capability",
  };
  let saved = false;
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    if (url === "/api/v1/apiary/keeper-links" && init?.method === "POST") {
      expect(JSON.parse(String(init.body))).toEqual(capability);
      saved = true;
      return ok({
        link: keeperLink("awaiting_approval"),
        invitation_received: false,
      }, 201);
    }
    if (url === "/api/v1/apiary/keeper-links") return ok(saved ? [keeperLink("awaiting_approval")] : []);
    if (url === "/api/v1/apiary/join-invitations") return ok([]);
    throw new Error(`Unexpected request: ${url}`);
  }));

  render(<ApiarySettings busy={false} hiveIdentity={personalIdentity()} operatorToken="secret" onHiveIdentityChange={vi.fn()} />);
  const joinGuide = screen.getByRole("list", { name: "How this Hive joins an Apiary" });
  expect(joinGuide).toHaveTextContent("Hand the link to this Hive");
  expect(joinGuide).toHaveTextContent("Wait for her approval");
  expect(joinGuide).toHaveTextContent("Review and join");
  fireEvent.change(screen.getByLabelText("Keeper invitation link"), {
    target: { value: createApiaryHandoffLink("keeper", capability, capability.keeper_endpoint) },
  });
  fireEvent.click(screen.getByRole("button", { name: "Connect to Keeper" }));

  expect(await screen.findByRole("status")).toHaveTextContent(/introduced itself.*Waiting for the Keeper/i);
  expect(screen.getByRole("list", { name: "Pending Keeper invitations" })).toHaveTextContent("Waiting for Keeper approval");
  expect(screen.getByRole("note")).toHaveTextContent("This Hive continues polling Jira directly as you");
  expect(screen.getByRole("note")).toHaveTextContent("polls the Keeper for shared Apiary tasks");
});

test("prefills a Keeper link handed off from the public invitation landing without browser storage", async () => {
  const capability = { link_id: "link-2", keeper_endpoint: "https://keeper.example.test", secret: "one-use" };
  const link = createApiaryHandoffLink("keeper", capability, capability.keeper_endpoint);
  stageApiaryHandoff("keeper", link);
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL) => {
    const url = String(input);
    if (url === "/api/v1/apiary/keeper-links" || url === "/api/v1/apiary/join-invitations") return ok([]);
    throw new Error(`Unexpected request: ${url}`);
  }));

  render(<ApiarySettings busy={false} hiveIdentity={personalIdentity()} operatorToken="secret" onHiveIdentityChange={vi.fn()} />);

  expect(await screen.findByLabelText("Keeper invitation link")).toHaveValue(link);
  expect(window.sessionStorage.length).toBe(0);
  expect(window.localStorage.getItem("swarm-next-apiary-keeper")).toBeNull();
});

test("restores a pending Keeper capability after the browser is reopened", async () => {
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL) => {
    const url = String(input);
    if (url === "/api/v1/apiary/keeper-links") return ok([keeperLink("awaiting_approval")]);
    if (url === "/api/v1/apiary/join-invitations") return ok([]);
    throw new Error(`Unexpected request: ${url}`);
  }));

  render(<ApiarySettings busy={false} hiveIdentity={personalIdentity()} operatorToken="secret" onHiveIdentityChange={vi.fn()} />);

  const pending = await screen.findByRole("list", { name: "Pending Keeper invitations" });
  expect(pending).toHaveTextContent("Wildflower Garden");
  expect(pending).toHaveTextContent("https://keeper.example.test");
  expect(pending).toHaveTextContent("Waiting for Keeper approval");
});

test("explains and dismisses a Keeper-cancelled invitation", async () => {
  let removed = false;
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    if (url === "/api/v1/apiary/keeper-links/link-1" && init?.method === "DELETE") {
      removed = true;
      return new Response(null, { status: 204 });
    }
    if (url === "/api/v1/apiary/keeper-links") return ok(removed ? [] : [keeperLink("revoked")]);
    if (url === "/api/v1/apiary/join-invitations") return ok([]);
    throw new Error(`Unexpected request: ${url}`);
  }));

  render(<ApiarySettings busy={false} hiveIdentity={personalIdentity()} operatorToken="secret" onHiveIdentityChange={vi.fn()} />);

  const pending = await screen.findByRole("list", { name: "Pending Keeper invitations" });
  expect(pending).toHaveTextContent("Cancelled by Keeper");
  fireEvent.click(screen.getByRole("button", { name: "Dismiss" }));
  expect(screen.getByRole("group", { name: "Confirm saved invitation removal" })).toHaveTextContent("Remove linkKeep waiting");
  fireEvent.click(screen.getByRole("button", { name: "Remove link" }));

  expect(await screen.findByRole("status")).toHaveTextContent("cancelled or expired invitation was removed");
  expect(screen.queryByRole("list", { name: "Pending Keeper invitations" })).not.toBeInTheDocument();
});

test("renames the local Hive without changing its durable identity", async () => {
  const onHiveIdentityChange = vi.fn();
  const renamed = personalIdentity();
  renamed.hive.name = "Clover Hive";
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    if (url.endsWith("/api/v1/apiary/keeper-links")) return ok([]);
    if (url.endsWith("/api/v1/apiary/join-invitations")) return ok([]);
    if (url === "/api/v1/hive" && init?.method === "PUT") {
      expect(JSON.parse(String(init.body))).toEqual({ name: "Clover Hive" });
      return ok(renamed);
    }
    throw new Error(`Unexpected request: ${url}`);
  }));

  render(<ApiarySettings busy={false} hiveIdentity={personalIdentity()} operatorToken="secret" onHiveIdentityChange={onHiveIdentityChange} />);
  fireEvent.click(screen.getByRole("button", { name: "Edit names" }));
  expect(screen.getByRole("group", { name: "Hive and Apiary names" })).toHaveTextContent(/repositories, tasks, Jira projects, and federation keys do not change/i);
  fireEvent.change(screen.getByLabelText("Hive name"), { target: { value: "  Clover Hive  " } });
  fireEvent.click(screen.getByRole("button", { name: "Save Hive name" }));

  await vi.waitFor(() => expect(onHiveIdentityChange).toHaveBeenCalledWith(renamed));
  expect(await screen.findByRole("status")).toHaveTextContent("This Hive is now named Clover Hive");
});

afterEach(() => {
  cleanup();
  clearStagedApiaryHandoff("keeper");
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

test("Keeper creates one private invitation link for an outbound member connection", async () => {
  const writeText = vi.fn().mockResolvedValue(undefined);
  vi.stubGlobal("navigator", { ...navigator, clipboard: { writeText } });
  let created = false;
  let revoked = false;
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    if (url === "/api/v1/apiary/join-links" && init?.method === "POST") {
      created = true;
      return ok({
        link: apiaryJoinLink("open"),
        one_time_secret: "shown-once",
      }, 201);
    }
    if (url === "/api/v1/apiary/join-links/link-1" && init?.method === "DELETE") {
      revoked = true;
      return ok(apiaryJoinLink("revoked"));
    }
    if (url === "/api/v1/apiary/join-links") return ok(created && !revoked ? [apiaryJoinLink("open")] : []);
    if (["/api/v1/apiary/jira-projects", "/api/v1/integrations/jira/bindings", "/api/v1/apiary/shared-work", "/api/v1/apiary/stewardships", "/api/v1/apiary/members"].includes(url)) return ok([]);
    if (url === "/api/v1/apiary/collapse-readiness") return ok({ active_hive_count: 1, pending_invitation_count: 0, active_stewardship_count: 0, open_cross_hive_work_count: 0, departed_node_count: 0 });
    throw new Error(`unexpected request ${url}`);
  }));

  render(<ApiarySettings busy={false} hiveIdentity={keeperIdentity()} operatorToken="secret" onHiveIdentityChange={vi.fn()} />);
  fireEvent.click(screen.getByRole("button", { name: "Create invitation link" }));

  expect(await screen.findByRole("status")).toHaveTextContent("Invitation link copied");
  expect(writeText).toHaveBeenCalledWith(expect.stringMatching(/^https:\/\/keeper\.example\.test\/#swarm-next-apiary-keeper=/));
  expect(screen.getByRole("group", { name: "Created Apiary link" })).toHaveTextContent(/private invitation link/i);
  expect(screen.getByRole("list", { name: "How the personal Hive uses this invitation" })).toHaveTextContent(/She opens the link.*Her Hive connects outward.*You approve the exact Hive/i);
  expect(screen.getByRole("group", { name: "Created Apiary link" })).toHaveTextContent(/paste the complete link into Settings.*Apiary.*Join a Keeper's Apiary/i);
  expect(screen.getByRole("note")).toHaveTextContent("Each Hive polls Jira directly");
  expect(screen.getByRole("note")).toHaveTextContent("Member Hives poll this Keeper");
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  const confirmation = screen.getByRole("group", { name: "Confirm invitation cancellation" });
  expect(confirmation).toHaveTextContent("Cancel invitation");
  expect(confirmation).toHaveTextContent("Keep link");
  fireEvent.click(screen.getByRole("button", { name: "Cancel invitation" }));
  expect(await screen.findByRole("status")).toHaveTextContent("Invitation cancelled");
  expect(screen.queryByRole("list", { name: "Apiary invitation links" })).not.toBeInTheDocument();
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

test("shows registered Apiary Hives without implying live presence", async () => {
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL) => {
    const url = String(input);
    if (url === "/api/v1/apiary/members") return ok([
      { hive_id: "hive-1", hive_name: "Meadow Hive", operator_id: "operator-1", operator_display_name: "Bea", role: "keeper", is_local: true },
      { hive_id: "hive-2", hive_name: "Clover Hive", operator_id: "operator-2", operator_display_name: "Cora", role: "member", is_local: false },
    ]);
    if (url === "/api/v1/apiary/collapse-readiness") {
      return ok({ active_hive_count: 2, pending_invitation_count: 0, active_stewardship_count: 0, open_cross_hive_work_count: 0, departed_node_count: 0 });
    }
    if (url === "/api/v1/apiary/jira-projects" || url === "/api/v1/integrations/jira/bindings" || url === "/api/v1/apiary/hive-candidates") return ok([]);
    throw new Error(`unexpected request ${url}`);
  }));

  render(<ApiarySettings busy={false} hiveIdentity={keeperIdentity()} operatorToken="secret" onHiveIdentityChange={vi.fn()} />);

  const roster = await screen.findByRole("list", { name: "Apiary Hives" });
  expect(roster).toHaveTextContent("Meadow HiveBeaKeeperThis Hive");
  expect(roster).toHaveTextContent("Clover HiveCoraMember");
  expect(screen.getByText(/Registered membership, not live presence/)).toBeVisible();
});

test("does not present a failed Apiary roster refresh as missing membership", async () => {
  let memberAttempts = 0;
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL) => {
    const url = String(input);
    if (url === "/api/v1/apiary/members") {
      memberAttempts += 1;
      if (memberAttempts === 1) return new Response("keeper unavailable", { status: 502 });
      return ok([
        { hive_id: "hive-1", hive_name: "Meadow Hive", operator_id: "operator-1", operator_display_name: "Bea", role: "keeper", is_local: true },
        { hive_id: "hive-2", hive_name: "Clover Hive", operator_id: "operator-2", operator_display_name: "Cora", role: "member", is_local: false },
      ]);
    }
    if (url === "/api/v1/apiary/collapse-readiness") return ok({ active_hive_count: 2, pending_invitation_count: 0, active_stewardship_count: 0, open_cross_hive_work_count: 0, departed_node_count: 0 });
    if (["/api/v1/apiary/jira-projects", "/api/v1/integrations/jira/bindings", "/api/v1/apiary/shared-work", "/api/v1/apiary/stewardships", "/api/v1/apiary/hive-candidates"].includes(url)) return ok([]);
    throw new Error(`unexpected request ${url}`);
  }));

  render(<ApiarySettings busy={false} hiveIdentity={keeperIdentity()} operatorToken="secret" onHiveIdentityChange={vi.fn()} />);

  expect(await screen.findByText("The Hive roster could not be refreshed. Last-known membership remains unchanged.")).toBeInTheDocument();
  expect(screen.queryByText("Membership is being refreshed.")).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Retry Hive roster" }));
  expect(await screen.findByText("Clover Hive")).toBeInTheDocument();
  expect(screen.queryByText("The Hive roster could not be refreshed. Last-known membership remains unchanged.")).not.toBeInTheDocument();
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

test("does not invite a duplicate promotion when the catalog refresh fails afterward", async () => {
  let promoted = false;
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    if (url === "/api/v1/apiary/collapse-readiness") return ok({ active_hive_count: 1, pending_invitation_count: 0, active_stewardship_count: 0, open_cross_hive_work_count: 0, departed_node_count: 0 });
    if (url === "/api/v1/apiary/hive-candidates") return ok([]);
    if (url === "/api/v1/apiary/jira-projects" && !init?.method) {
      if (promoted) return new Response("temporary outage", { status: 502 });
      return ok([]);
    }
    if (url === "/api/v1/integrations/jira/bindings") return ok([{
      id: "binding-1", project_id: "10001", project_key: "WEB", project_name: "Website Services",
      scope: promoted ? "apiary" : "hive", hive_id: "hive-1", apiary_id: promoted ? "apiary-1" : null,
      access_verified: true, workflow_mapped: true, auto_sync_assigned: true,
    }]);
    if (url === "/api/v1/apiary/jira-projects/binding-1/promotion") {
      promoted = true;
      return ok({ apiary_id: "apiary-1", project_id: "10001", project_key: "WEB", project_name: "Website Services", promoted_by_operator_id: "operator-1", promoted_at: 20 }, 201);
    }
    throw new Error(`unexpected request ${url}`);
  }));

  render(<ApiarySettings busy={false} hiveIdentity={keeperIdentity()} operatorToken="secret" onHiveIdentityChange={vi.fn()} />);
  fireEvent.click(await screen.findByRole("button", { name: "Promote project" }));

  expect(await screen.findByRole("status")).toHaveTextContent("WEB is now in the Apiary project catalog");
  expect(screen.getByText(/WEB was promoted successfully/)).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Promote project" })).not.toBeInTheDocument();
});

test("Keeper approves the exact Hive and leaves delivery to its next outbound poll", async () => {
  let approved = false;
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    if (url === "/api/v1/apiary/join-links/link-1/approval") {
      expect(init?.method).toBe("POST");
      approved = true;
      return ok(apiaryJoinLink("approved", true));
    }
    if (url === "/api/v1/apiary/join-links") return ok([apiaryJoinLink(approved ? "approved" : "awaiting_approval", true)]);
    if (url === "/api/v1/apiary/collapse-readiness") return ok({ active_hive_count: 1, pending_invitation_count: approved ? 1 : 0, active_stewardship_count: 0, open_cross_hive_work_count: 0, departed_node_count: 0 });
    if (["/api/v1/apiary/jira-projects", "/api/v1/integrations/jira/bindings", "/api/v1/apiary/shared-work", "/api/v1/apiary/stewardships", "/api/v1/apiary/members"].includes(url)) return ok([]);
    throw new Error(`unexpected request ${url}`);
  }));

  render(<ApiarySettings busy={false} hiveIdentity={keeperIdentity()} operatorToken="secret" onHiveIdentityChange={vi.fn()} />);
  const approvals = await screen.findByRole("region", { name: "Hives waiting for approval" });
  expect(approvals).toHaveTextContent("Clover Hive");
  expect(approvals).toHaveTextContent("Cora · identity verified");
  fireEvent.click(screen.getByRole("button", { name: "Approve Hive" }));

  expect(await screen.findByRole("status")).toHaveTextContent("Her Hive will receive the signed invitation on its next outbound poll");
  expect(screen.getByRole("list", { name: "Apiary invitation links" })).toHaveTextContent("Approved · awaiting poll");
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
  let policyAccepted = false;
  const overview = () => ({
    ...imported,
    state: policyAccepted ? "policy_accepted" : "keeper_pinned",
    readiness: {
      jira_connection: "ready",
      projects: [{
        project: bundle.promoted_projects[0],
        binding_id: null,
        access_verified: false,
        workflow_mapped: false,
      }],
      blockers: policyAccepted ? ["project_access_not_ready"] : ["project_access_not_ready", "policy_not_accepted"],
    },
  });
  const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    if (url === "/api/v1/apiary/join-invitations" && init?.method === "POST") {
      expect(JSON.parse(String(init.body))).toEqual(bundle);
      saved = true;
      return ok(imported, 201);
    }
    if (url === "/api/v1/apiary/join-invitations/invite-1/policy-acceptance") {
      expect(init?.method).toBe("POST");
      expect(JSON.parse(String(init?.body))).toEqual({ policy_revision: 3 });
      policyAccepted = true;
      return ok(overview());
    }
    if (url === "/api/v1/apiary/join-invitations") return ok(saved ? [overview()] : []);
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
  const savedInvitations = screen.getByRole("list", { name: "Saved Apiary invitations" });
  expect(savedInvitations).toHaveTextContent("2 readiness steps left");
  expect(savedInvitations).toHaveTextContent("Connect this Jira project");
  expect(saved).toBe(true);
  fireEvent.click(screen.getByRole("button", { name: "Acknowledge revision 3" }));
  expect(await screen.findByRole("status")).toHaveTextContent("Policy revision 3 accepted locally");
  expect(savedInvitations).toHaveTextContent("Acknowledged");
  expect(savedInvitations).toHaveTextContent("1 readiness step left");
  expect(policyAccepted).toBe(true);
});

test("joins a ready Apiary through the member-initiated Keeper connection", async () => {
  const onHiveIdentityChange = vi.fn();
  const overview = {
    invitation_id: "invite-1", apiary_id: "apiary-1", apiary_name: "Wildflower Garden",
    shared_work_backend: "jira", required_policy_revision: 3,
    promoted_project_catalog_digest: "digest", promoted_projects: [],
    keeper_node_id: "keeper-node", keeper_hive_id: "keeper-hive", keeper_hive_name: "Rose Hive",
    keeper_operator_id: "keeper-operator", keeper_operator_display_name: "Rosa",
    keeper_endpoint: "https://keeper.example.test/swarm",
    state: "policy_accepted",
    imported_at: 20, expires_at: 86_410,
    readiness: { jira_connection: "ready", projects: [], blockers: [] },
  };
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    if (url === "/api/v1/apiary/join-invitations/invite-1/submission") {
      expect(init?.method).toBe("POST");
      return ok(memberIdentity().apiary_context, 201);
    }
    if (url === "/api/v1/apiary/join-invitations") return ok([overview]);
    if (url === "/api/v1/hive") return ok(memberIdentity());
    throw new Error(`unexpected request ${url}`);
  }));

  render(<ApiarySettings busy={false} hiveIdentity={personalIdentity()} operatorToken="secret" onHiveIdentityChange={onHiveIdentityChange} />);
  fireEvent.click(await screen.findByRole("button", { name: "Join Apiary" }));

  expect(await screen.findByRole("status")).toHaveTextContent("joined Wildflower Garden");
  expect(screen.getByRole("status")).toHaveTextContent("Jira continues syncing directly");
  expect(onHiveIdentityChange).toHaveBeenCalledWith(memberIdentity());
});

test("shows a low-noise Keeper rollup of reservations and durable ownership", async () => {
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL) => {
    const url = String(input);
    if (url === "/api/v1/apiary/members") return ok([
      { hive_id: "hive-1", hive_name: "Meadow Hive", operator_id: "operator-1", operator_display_name: "Bea", role: "keeper", is_local: true },
      { hive_id: "hive-2", hive_name: "Clover Hive", operator_id: "operator-2", operator_display_name: "Cora", role: "member", is_local: false },
    ]);
    if (url === "/api/v1/apiary/collapse-readiness") {
      return ok({ active_hive_count: 2, pending_invitation_count: 0, active_stewardship_count: 0, open_cross_hive_work_count: 2, departed_node_count: 0 });
    }
    if (url === "/api/v1/apiary/jira-projects") return ok([]);
    if (url === "/api/v1/integrations/jira/bindings") return ok([]);
    if (url === "/api/v1/apiary/hive-candidates") return ok([]);
    if (url === "/api/v1/apiary/shared-work") return ok([
      {
        id: "claim-1", apiary_id: "apiary-1", project_id: "10001", issue_id: "20001", issue_key: "WWD-101",
        home_node_id: "node-2", home_hive_id: "hive-2", home_operator_id: "operator-2", state: "reserved",
        reserved_at: 100, reservation_expires_at: 220, confirmed_at: null, released_at: null,
        project_key: "WWD", project_name: "Website Development", home_hive_name: "Clover Hive", home_operator_display_name: "Cora",
      },
      {
        id: "claim-2", apiary_id: "apiary-1", project_id: "10001", issue_id: "20002", issue_key: "WWD-102",
        home_node_id: "node-1", home_hive_id: "hive-1", home_operator_id: "operator-1", state: "confirmed",
        reserved_at: 90, reservation_expires_at: 210, confirmed_at: 110, released_at: null,
        project_key: "WWD", project_name: "Website Development", home_hive_name: "Meadow Hive", home_operator_display_name: "Bea",
      },
    ]);
    throw new Error(`unexpected request ${url}`);
  }));

  render(<ApiarySettings busy={false} hiveIdentity={keeperIdentity()} operatorToken="secret" onHiveIdentityChange={vi.fn()} />);

  const rollup = await screen.findByRole("list", { name: "Apiary shared work ownership" });
  expect(rollup).toHaveTextContent("WWD-101Website DevelopmentClover HiveCoraClaiming");
  expect(rollup).toHaveTextContent("WWD-102Website DevelopmentMeadow HiveBeaOwned");
  expect(screen.getByText(/Routine worker activity stays inside each Hive/)).toBeVisible();
  expect(rollup).not.toHaveTextContent("credential");
});

test("delegates and revokes explicit Steward authority without exposing Hive internals", async () => {
  let stewardships: Array<Record<string, unknown>> = [];
  const members = [
    { hive_id: "hive-1", hive_name: "Meadow Hive", operator_id: "operator-1", operator_display_name: "Bea", role: "keeper", is_local: true },
    { hive_id: "hive-2", hive_name: "IT Lead", operator_id: "operator-2", operator_display_name: "Tessa", role: "member", is_local: false },
    { hive_id: "hive-3", hive_name: "IT Support", operator_id: "operator-3", operator_display_name: "Ivy", role: "member", is_local: false },
  ];
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    if (url === "/api/v1/apiary/members") return ok(members);
    if (url === "/api/v1/apiary/collapse-readiness") return ok({ active_hive_count: 3, pending_invitation_count: 0, active_stewardship_count: stewardships.length, open_cross_hive_work_count: 0, departed_node_count: 0 });
    if (url === "/api/v1/apiary/stewardships" && !init?.method) return ok(stewardships);
    if (url === "/api/v1/apiary/stewardships/by-operator/operator-2") {
      expect(init?.method).toBe("PUT");
      expect(JSON.parse(String(init?.body))).toEqual({
        managed_hive_ids: ["hive-2", "hive-3"],
        capabilities: ["observe", "assign", "assist", "takeover"],
      });
      const saved = {
        id: "stewardship-1", apiary_id: "apiary-1", steward_operator_id: "operator-2",
        managed_hive_ids: ["hive-2", "hive-3"], capabilities: ["observe", "assign", "assist", "takeover"],
      };
      stewardships = [saved];
      return ok(saved);
    }
    if (url === "/api/v1/apiary/stewardships/stewardship-1") {
      expect(init?.method).toBe("DELETE");
      stewardships = [];
      return new Response(null, { status: 204 });
    }
    if (["/api/v1/apiary/jira-projects", "/api/v1/integrations/jira/bindings", "/api/v1/apiary/hive-candidates", "/api/v1/apiary/shared-work"].includes(url)) return ok([]);
    throw new Error(`unexpected request ${url}`);
  }));

  render(<ApiarySettings busy={false} hiveIdentity={keeperIdentity()} operatorToken="secret" onHiveIdentityChange={vi.fn()} />);
  fireEvent.click(await screen.findByRole("button", { name: "Delegate a Steward" }));
  const editor = screen.getByRole("group", { name: "Steward delegation" });
  expect(editor).toHaveTextContent("Tessa · IT Lead");
  fireEvent.click(screen.getByLabelText(/IT SupportIvy/));
  fireEvent.click(screen.getByRole("button", { name: "Save Stewardship" }));

  const list = await screen.findByRole("list", { name: "Apiary Stewards" });
  expect(list).toHaveTextContent("TessaIT Lead");
  expect(list).toHaveTextContent("IT Lead, IT Support");
  expect(list).toHaveTextContent("See statusRoute workAssistTake over");
  expect(list.textContent).not.toMatch(/credential|repository|terminal/i);
  fireEvent.click(screen.getByRole("button", { name: "Revoke" }));
  fireEvent.click(screen.getByRole("button", { name: "Confirm revoke" }));
  expect(await screen.findByRole("status")).toHaveTextContent("Tessa is no longer a Steward");
  expect(screen.queryByRole("list", { name: "Apiary Stewards" })).not.toBeInTheDocument();
});

test("shows honest Member convergence while waiting for the first automatic poll", async () => {
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL) => {
    const url = String(input);
    if (url === "/api/v1/apiary/members") return ok([
      { hive_id: "hive-1", hive_name: "Meadow Hive", operator_id: "operator-1", operator_display_name: "Bea", role: "keeper", is_local: false },
      { hive_id: "hive-2", hive_name: "Clover Hive", operator_id: "operator-2", operator_display_name: "Cora", role: "member", is_local: true },
    ]);
    if (url === "/api/v1/apiary/sync-health") return ok({
      condition: "idle", last_attempt_at: null, last_success_at: null, consecutive_failures: 0, next_attempt_at: null,
    });
    if (url === "/api/v1/apiary/catalog-readiness") return ok({
      acknowledgement: null,
      jira_connection: "ready",
      projects: [{
        project: { project_id: "10001", project_key: "WWD", project_name: "Website Development" },
        binding_id: "binding-1", access_verified: true, workflow_mapped: true,
      }],
      blockers: ["catalog_missing"],
    });
    if (url === "/api/v1/apiary/departure-readiness") return ok(departureStatus());
    throw new Error(`unexpected request ${url}`);
  }));

  render(<ApiarySettings busy={false} hiveIdentity={memberIdentity()} operatorToken="secret" onHiveIdentityChange={vi.fn()} />);

  const status = await screen.findByLabelText("Keeper synchronization status");
  expect(status).toHaveTextContent("Waiting for first sync");
  expect(status).toHaveTextContent("This Hive will poll Keeper automatically");
  expect(status).toHaveTextContent("CatalogWaitingProjects ready1/1JiraConnectedRetries0");
  expect(status).toHaveTextContent("Shared work waits for: catalog missing");
  expect(status).not.toHaveTextContent("credential");
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

function memberIdentity(): HiveIdentity {
  return {
    operator: { id: "operator-2", display_name: "Cora" },
    hive: { id: "hive-2", name: "Clover Hive", operator_id: "operator-2", apiary_id: "apiary-1" },
    apiary_context: {
      mode: "federated",
      apiary: { id: "apiary-1", name: "Wildflower Garden", keeper_operator_id: "operator-1", shared_work_backend: "jira" },
      local_role: "member",
    },
  };
}

function departureStatus() {
  return {
    state: "active",
    keeper_reachable: true,
    readiness: {
      apiary_id: "apiary-1",
      member_node_id: "node-2",
      member_hive_id: "hive-2",
      active_jira_claim_count: 0,
      open_swarm_task_count: 0,
      active_stewardship_count: 0,
      pending_task_command_count: 0,
      pending_jira_claim_count: 0,
    },
  };
}

function keeperLink(state: "contacting" | "awaiting_approval" | "revoked") {
  return {
    link_id: "link-1",
    keeper_endpoint: "https://keeper.example.test",
    apiary_id: "apiary-1",
    apiary_name: "Wildflower Garden",
    state,
    created_at: 10,
    expires_at: 86_410,
    last_poll_at: null,
    last_error: null,
  };
}

function apiaryJoinLink(state: "open" | "awaiting_approval" | "approved" | "invitation_issued" | "revoked", withCandidate = false) {
  return {
    id: "link-1",
    apiary_id: "apiary-1",
    apiary_name: "Wildflower Garden",
    keeper_endpoint: "https://keeper.example.test",
    state,
    candidate: withCandidate ? {
      node_id: "node-2",
      hive_id: "hive-2",
      hive_name: "Clover Hive",
      operator_id: "operator-2",
      operator_display_name: "Cora",
      public_key: "public",
    } : null,
    issued_at: 10,
    expires_at: 86_410,
  };
}

function ok(body: unknown, status = 200) {
  return new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json" } });
}

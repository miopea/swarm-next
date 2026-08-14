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

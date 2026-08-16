import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import MemberDeparturePanel from "./MemberDeparturePanel";

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

test("explains preserved data and requires the Apiary name before leaving", async () => {
  const onLeft = vi.fn(async () => undefined);
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    if (url === "/api/v1/apiary/departure-readiness") return ok(status());
    if (url === "/api/v1/apiary/departure" && init?.method === "POST") return ok({ mode: "personal" });
    throw new Error(`unexpected request ${url}`);
  }));

  render(<MemberDeparturePanel apiaryName="Wildflower Garden" busy={false} operatorToken="secret" onLeft={onLeft} />);

  expect(await screen.findByText("Workers, repositories, provider conversations, private tasks, settings, and Hive-owned integrations.")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Review leaving Apiary" }));
  const leave = screen.getByRole("button", { name: "Leave Apiary" });
  expect(leave).toBeDisabled();
  fireEvent.change(screen.getByLabelText("Type Wildflower Garden to confirm"), { target: { value: "Wildflower Garden" } });
  fireEvent.click(leave);

  await vi.waitFor(() => expect(onLeft).toHaveBeenCalledOnce());
});

test("blocks departure while shared work remains", async () => {
  vi.stubGlobal("fetch", vi.fn(async () => ok(status({ active_jira_claim_count: 2, open_swarm_task_count: 1 }))));
  render(<MemberDeparturePanel apiaryName="Wildflower Garden" busy={false} operatorToken="secret" onLeft={vi.fn()} />);

  expect(await screen.findByText("Clear before leaving: 2 active Jira claims, 1 open Apiary task.")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Review leaving Apiary" })).toBeDisabled();
});

test("makes a frozen transport retry explicit and recoverable", async () => {
  vi.stubGlobal("fetch", vi.fn(async () => ok({ ...status(), state: "departing" })));
  render(<MemberDeparturePanel apiaryName="Wildflower Garden" busy={false} operatorToken="secret" onLeft={vi.fn()} />);

  expect(await screen.findByText("Departure paused safely")).toBeInTheDocument();
  expect(screen.getByText(/No partial departure occurred/)).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Retry departure" })).toBeEnabled();
});

function status(overrides: Record<string, number> = {}) {
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
      ...overrides,
    },
  };
}

function ok(body: unknown) {
  return Promise.resolve(new Response(JSON.stringify(body), {
    status: 200,
    headers: { "Content-Type": "application/json" },
  }));
}

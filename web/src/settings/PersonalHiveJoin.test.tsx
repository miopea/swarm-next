import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import PersonalHiveJoin from "./PersonalHiveJoin";
import { createApiaryHandoffLink } from "./apiaryHandoff";

test("signed link shows terms before one submission and no second acceptance", async () => {
  const offer = { payload: { schema_version: 1, link_id: "link-1", apiary_id: "garden-1",
    apiary_name: "Fictional Garden", keeper_endpoint: "https://keeper.example.test",
    keeper: { payload: { operator_display_name: "Bea", hive_name: "Keeper Hive" } },
    policy_revision: 4, management_terms_version: 1, issued_at: 10, expires_at: 3600 }, signature: "fictional" };
  const record = { consent: { link_id: "link-1", apiary_id: "garden-1", policy_revision: 4, expires_at: 3600 }, phase: "awaiting_approval" };
  const actions: string[] = [];
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    if (url.endsWith("/enrollments") && init?.method === "POST") {
      actions.push("submit");
      expect(JSON.parse(String(init.body))).toEqual({ offer, secret: "fictional-secret" });
      return new Response(JSON.stringify(record), { status: 202 });
    }
    return new Response("[]");
  }));
  render(<PersonalHiveJoin busy={false} operatorToken="test" onError={vi.fn()} onMessage={vi.fn()} onJoined={vi.fn()} />);
  fireEvent.change(screen.getByLabelText("Keeper invitation link"), { target: { value:
    createApiaryHandoffLink("keeper", { link_id: "link-1", keeper_endpoint: "https://keeper.example.test",
      secret: "fictional-secret", enrollment_offer: offer }, "https://keeper.example.test") } });
  expect(screen.getByText(/Submitting accepts policy revision 4/)).toBeInTheDocument();
  expect(screen.queryByText("Review before joining")).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Request to join" }));
  await waitFor(() => expect(screen.getByRole("heading", { name: "Waiting for Keeper approval" })).toBeInTheDocument());
  expect(actions).toEqual(["submit"]);
  expect(screen.queryByRole("button", { name: /Accept policy|Join Apiary/ })).not.toBeInTheDocument();
});

test("completed enrollment can recover a failed view refresh without submitting again", async () => {
  const requests = vi.fn(async (input: RequestInfo | URL, _init?: RequestInit) => new Response(
    String(input).endsWith("/enrollments") ? JSON.stringify([{ consent: { link_id: "link-1" }, phase: "complete" }]) : "[]"));
  vi.stubGlobal("fetch", requests);
  const joined = vi.fn().mockRejectedValueOnce(new Error("refresh unavailable")).mockResolvedValue(undefined);
  render(<PersonalHiveJoin busy={false} operatorToken="test" onError={vi.fn()} onMessage={vi.fn()} onJoined={joined} />);
  const retry = await screen.findByRole("button", { name: "Open Apiary" });
  expect(screen.getByRole("alert")).toHaveTextContent("Your membership is saved");
  fireEvent.click(retry);
  await waitFor(() => expect(joined).toHaveBeenCalledTimes(2));
  expect(screen.queryByRole("button", { name: "Open Apiary" })).not.toBeInTheDocument();
  expect(requests.mock.calls.every(([, init]) => !init?.method || init.method === "GET")).toBe(true);
});

test("joining status pauses while hidden and cancels its read when leaving", async () => {
  let visibility: DocumentVisibilityState = "hidden";
  const visibilitySpy = vi.spyOn(document, "visibilityState", "get").mockImplementation(() => visibility);
  let signal: AbortSignal | null | undefined;
  let enrollmentReads = 0;
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    if (!String(input).endsWith("/enrollments")) return new Response("[]");
    enrollmentReads += 1;
    signal = init?.signal;
    return new Promise<Response>((_resolve, reject) => signal?.addEventListener("abort", () => reject(new DOMException("Cancelled", "AbortError")), { once: true }));
  }));
  try {
    await act(async () => { render(<PersonalHiveJoin busy={false} operatorToken="test" onError={vi.fn()} onMessage={vi.fn()} onJoined={vi.fn()} />); });
    expect(enrollmentReads).toBe(0);
    visibility = "visible";
    await act(async () => { document.dispatchEvent(new Event("visibilitychange")); });
    expect(enrollmentReads).toBe(1);
    expect(signal?.aborted).toBe(false);
    visibility = "hidden";
    await act(async () => { document.dispatchEvent(new Event("visibilitychange")); });
    expect(signal?.aborted).toBe(true);
    expect(enrollmentReads).toBe(1);
  } finally { cleanup(); visibilitySpy.mockRestore(); }
});

test("saved completed enrollment opens Apiary without another member action", async () => {
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL) => new Response(
    String(input).endsWith("/enrollments") ? JSON.stringify([{ consent: { link_id: "link-1" }, phase: "complete" }]) : "[]")));
  const joined = vi.fn().mockResolvedValue(undefined);
  render(<PersonalHiveJoin busy={false} operatorToken="test" onError={vi.fn()} onMessage={vi.fn()} onJoined={joined} />);
  await waitFor(() => expect(joined).toHaveBeenCalledTimes(1));
  expect(screen.queryByRole("button", { name: /Accept policy|Join Apiary/ })).not.toBeInTheDocument();
});

test.each([
  ["keeper_unavailable", "awaiting_approval", /Keeper is temporarily unreachable/],
  ["invitation_unavailable", "attention", /This invitation expired or was cancelled/],
  ["approval_changed", "attention", /no longer matches your submitted terms/],
  ["runtime_incompatible", "attention", /could not agree on the joining protocol/],
] as const)("saved %s explains the next step without another approval", async (problem, phase, message) => {
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL) => new Response(
    String(input).endsWith("/enrollments") ? JSON.stringify([{ consent: { link_id: "link-1" }, phase, problem }]) : "[]")));
  render(<PersonalHiveJoin busy={false} operatorToken="test" onError={vi.fn()} onMessage={vi.fn()} onJoined={vi.fn()} />);
  await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent(message));
  expect(screen.queryByRole("button", { name: /Accept policy|Join Apiary/ })).not.toBeInTheDocument();
});

// Profile persistence is covered independently; these tests isolate join policy
// and membership failure/recovery rather than mocking its HTTP contract twice.
vi.mock("./JoinPublicProfile", async () => {
  const { useImperativeHandle } = await import("react");
  return { default: ({ ref }: { ref: import("react").Ref<{ save: () => Promise<void> }> }) => {
    useImperativeHandle(ref, () => ({ save: async () => undefined }));
    return null;
  } };
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

test.each(["ready", "readiness changed", "acceptance failed", "submission failed"])("explicit accept and join: %s", async (outcome) => {
  const invitation = {
    invitation_id: "invite-1", apiary_name: "Clover Garden", keeper_hive_name: "Lead Hive",
    keeper_operator_display_name: "Bea", required_policy_revision: 3, promoted_projects: [],
    state: "keeper_pinned", readiness: { jira_connection: "ready", projects: [], blockers: ["policy_not_accepted"] },
  };
  const actions: string[] = [];
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    if (url.endsWith("/policy-acceptance") && init?.method === "POST") {
      actions.push("accept");
      expect(JSON.parse(String(init.body))).toEqual({ policy_revision: 3 });
      if (outcome === "acceptance failed") return new Response("acceptance unavailable", { status: 503 });
      return new Response(JSON.stringify({ ...invitation, state: "policy_accepted", readiness: {
        ...invitation.readiness, blockers: outcome === "readiness changed" ? ["project_access_not_ready"] : [],
      } }), { status: 200 });
    }
    if (url.endsWith("/submission") && init?.method === "POST") {
      actions.push("join");
      return outcome === "submission failed"
        ? new Response("Keeper unavailable", { status: 503 })
        : new Response(JSON.stringify({ kind: "federated" }), { status: 200 });
    }
    return new Response(JSON.stringify(url.endsWith("/join-invitations") ? [invitation] : []), { status: 200 });
  }));
  const onError = vi.fn();
  const onMessage = vi.fn();
  const onJoined = vi.fn().mockResolvedValue(undefined);
  render(<PersonalHiveJoin busy={false} operatorToken="fictional" onError={onError} onMessage={onMessage} onJoined={onJoined} />);
  const button = await screen.findByRole("button", { name: "Accept policy and join" });
  expect(actions).toEqual([]);
  fireEvent.click(button);
  if (outcome === "ready") {
    expect(await screen.findByText("Joined Clover Garden")).toBeInTheDocument();
    expect(actions).toEqual(["accept", "join"]);
    expect(onJoined).toHaveBeenCalledOnce();
  } else {
    if (outcome === "readiness changed") {
      await waitFor(() => expect(onMessage).toHaveBeenCalledWith(expect.stringContaining("Readiness changed")));
    } else {
      await waitFor(() => expect(onError.mock.calls.some(([message]) => Boolean(message))).toBe(true));
    }
    expect(actions).toEqual(outcome === "submission failed" ? ["accept", "join"] : ["accept"]);
    expect(onJoined).not.toHaveBeenCalled();
    expect(screen.queryByText("Joined Clover Garden")).not.toBeInTheDocument();
    if (outcome === "submission failed") expect(screen.getByRole("button", { name: "Join Apiary" })).toBeEnabled();
  }
});

test("confirmed join remains successful when refreshing the membership view fails", async () => {
  const invitation = {
    invitation_id: "invite-1", apiary_name: "Clover Garden", keeper_hive_name: "Lead Hive",
    keeper_operator_display_name: "Bea", required_policy_revision: 3, promoted_projects: [],
    state: "policy_accepted", readiness: { jira_connection: "ready", projects: [], blockers: [] },
  };
  let submissions = 0;
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    if (url.endsWith("/submission") && init?.method === "POST") {
      submissions += 1;
      return new Response(JSON.stringify({ kind: "federated" }), { status: 200 });
    }
    return new Response(JSON.stringify(url.endsWith("/join-invitations") ? [invitation] : []), { status: 200 });
  }));
  const onError = vi.fn();
  const onMessage = vi.fn();
  render(<PersonalHiveJoin busy={false} operatorToken="fictional" onError={onError} onMessage={onMessage}
    onJoined={vi.fn().mockRejectedValue(new Error("refresh unavailable"))} />);
  fireEvent.click(await screen.findByRole("button", { name: "Join Apiary" }));
  expect(await screen.findByText("Joined Clover Garden")).toBeInTheDocument();
  await waitFor(() => expect(onError).toHaveBeenCalledWith(expect.stringContaining("You joined successfully")));
  expect(onMessage).toHaveBeenCalledWith(expect.stringContaining("joined Clover Garden"));
  expect(screen.queryByRole("button", { name: "Join Apiary" })).not.toBeInTheDocument();
  expect(submissions).toBe(1);
});

test("does not describe unavailable saved invitations as empty and retries them", async () => {
  let unavailable = true;
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL) => {
    const url = String(input);
    if (unavailable && url.endsWith("/api/v1/apiary/keeper-links")) return new Response("unavailable", { status: 502 });
    return new Response(JSON.stringify([]), { status: 200, headers: { "Content-Type": "application/json" } });
  }));

  render(<PersonalHiveJoin busy={false} operatorToken="secret" onError={vi.fn()} onMessage={vi.fn()} onJoined={vi.fn()} />);

  expect(await screen.findByText("Saved Keeper invitations could not be fully refreshed. Last-known links remain unchanged.")).toBeInTheDocument();
  unavailable = false;
  fireEvent.click(screen.getByRole("button", { name: "Retry saved invitations" }));
  await waitFor(() => expect(screen.queryByText(/Saved Keeper invitations could not be fully refreshed/)).not.toBeInTheDocument());
  expect(screen.getByText("No Apiary invitation is saved on this Hive.")).toBeInTheDocument();
});

test("keeps a pending invitation visible when the Keeper is temporarily unreachable", async () => {
  vi.useFakeTimers();
  const link = {
    link_id: "link-1",
    keeper_endpoint: "https://keeper.example.test",
    apiary_id: "apiary-1",
    apiary_name: "Wildflower Garden",
    state: "awaiting_approval",
    created_at: 10,
    expires_at: 86_410,
    last_poll_at: null,
    last_error: null,
  };
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    if (url.endsWith("/api/v1/apiary/keeper-links/link-1/poll") && init?.method === "POST") {
      return new Response("keeper unavailable", { status: 502 });
    }
    if (url.endsWith("/api/v1/apiary/keeper-links")) return new Response(JSON.stringify([link]), { status: 200, headers: { "Content-Type": "application/json" } });
    return new Response(JSON.stringify([]), { status: 200, headers: { "Content-Type": "application/json" } });
  }));

  render(<PersonalHiveJoin busy={false} operatorToken="secret" onError={vi.fn()} onMessage={vi.fn()} onJoined={vi.fn()} />);
  await vi.waitFor(() => expect(screen.getByText("Wildflower Garden")).toBeInTheDocument());

  await act(async () => { await vi.advanceTimersByTimeAsync(5_000); });

  await vi.waitFor(() => expect(screen.getByText("The Keeper was not reachable on the last check. This Hive keeps the invitation safely and retries every five seconds.")).toBeInTheDocument());
  expect(screen.getByText("Waiting for Keeper approval")).toBeInTheDocument();
});

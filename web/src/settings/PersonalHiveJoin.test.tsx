import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import PersonalHiveJoin from "./PersonalHiveJoin";

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

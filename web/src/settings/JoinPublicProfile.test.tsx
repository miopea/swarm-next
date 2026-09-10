import { createRef } from "react";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import JoinPublicProfile, { type JoinPublicProfileHandle } from "./JoinPublicProfile";
import PersonalHiveJoin from "./PersonalHiveJoin";

afterEach(() => { cleanup(); vi.unstubAllGlobals(); });

test.each([false, true])("joining saves the visible profile first; profile failure=%s", async (fails) => {
  const actions: string[] = [];
  const profile = { hive_name: "My Hive", operator_display_name: "Cora Bee", contact_email: null };
  vi.stubGlobal("fetch", vi.fn(async (input, init) => {
    const url = String(input);
    if (url.endsWith("/public-profile")) return new Response(JSON.stringify({ revision: 1, profile }));
    if (url.endsWith("/join-profile")) {
      actions.push("profile");
      return fails ? new Response("Unavailable", { status: 503 }) : new Response(JSON.stringify({ revision: 2, profile: { ...profile, hive_name: "Cora's Hive" } }));
    }
    if (url.endsWith("/submission") && init?.method === "POST") {
      actions.push("join"); return new Response(JSON.stringify({ kind: "federated" }));
    }
    return new Response(JSON.stringify(url.endsWith("/join-invitations") ? [{
      invitation_id: "fictional", apiary_name: "Clover Garden", keeper_hive_name: "Bea's Hive",
      keeper_operator_display_name: "Bea", required_policy_revision: 1, promoted_projects: [],
      state: "policy_accepted", readiness: { jira_connection: "ready", projects: [], blockers: [] },
    }] : []));
  }));
  const onError = vi.fn();
  render(<PersonalHiveJoin busy={false} operatorToken="fictional" onError={onError} onMessage={vi.fn()} onJoined={vi.fn()} />);
  await screen.findByLabelText("Your name");
  fireEvent.click(await screen.findByRole("button", { name: "Join Apiary" }));
  if (fails) {
    await waitFor(() => expect(onError).toHaveBeenCalledWith(expect.stringContaining("503")));
    expect(actions).toEqual(["profile"]);
    expect(screen.getByLabelText("Your name")).toHaveValue("Cora Bee");
  } else {
    await screen.findByText("Joined Clover Garden");
    expect(actions).toEqual(["profile", "join"]);
  }
});

test.each(["My Hive", "Clover House"])("previews and explicitly saves %s without publishing on load", async (hiveName) => {
  let writes = 0;
  const ref = createRef<JoinPublicProfileHandle>();
  vi.stubGlobal("fetch", vi.fn(async (_input, init) => {
    if (init?.method === "PUT") {
      writes++;
      expect(JSON.parse(init.body)).toEqual({ hive_name: hiveName, operator_display_name: "Cora Bee", contact_email: "cora@example.test" });
      return new Response(JSON.stringify({ revision: 2, profile: { ...JSON.parse(init.body), hive_name: hiveName === "My Hive" ? "Cora's Hive" : hiveName } }));
    }
    return new Response(JSON.stringify({ revision: 1, profile: { hive_name: hiveName, operator_display_name: "Operator", contact_email: null } }));
  }));
  render(<JoinPublicProfile ref={ref} operatorToken="fictional" disabled={false} />);
  fireEvent.change(await screen.findByLabelText("Your name"), { target: { value: "Cora Bee" } });
  fireEvent.change(screen.getByLabelText("Contact email (optional)"), { target: { value: "cora@example.test" } });
  expect(screen.getByLabelText("Shared profile preview")).toHaveTextContent(hiveName === "My Hive" ? "Cora's Hive" : hiveName);
  expect(writes).toBe(0);
  await act(() => ref.current!.save());
  expect(writes).toBe(1);
});

test("failed load can retry and failed saving rejects without discarding edits", async () => {
  let unavailable = true;
  const ref = createRef<JoinPublicProfileHandle>();
  vi.stubGlobal("fetch", vi.fn(async (_input, init) => {
    if (unavailable || init?.method === "PUT") return new Response("Unavailable", { status: 503 });
    return new Response(JSON.stringify({ revision: 1, profile: { hive_name: "My Hive", operator_display_name: "Cora", contact_email: null } }));
  }));
  render(<JoinPublicProfile ref={ref} operatorToken="fictional" disabled={false} />);
  await screen.findByRole("alert");
  await expect(ref.current!.save()).rejects.toThrow("has not loaded");
  unavailable = false;
  fireEvent.click(screen.getByRole("button", { name: "Retry profile" }));
  fireEvent.change(await screen.findByLabelText("Hive name"), { target: { value: "Clover House" } });
  await expect(ref.current!.save()).rejects.toThrow();
  expect(screen.getByLabelText("Hive name")).toHaveValue("Clover House");
  fireEvent.change(screen.getByLabelText("Your name"), { target: { value: "" } });
  await expect(ref.current!.save()).rejects.toThrow("Enter your name");
});

import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import type { KeeperPolicy } from "../api";
import ApiaryPolicyPanel from "./ApiaryPolicyPanel";

const ok = (body: unknown) => Promise.resolve(new Response(JSON.stringify(body), { headers: { "Content-Type": "application/json" } }));
const supported = [{ key: "release.auto_install", values: ["on", "off"] }];

afterEach(() => { vi.unstubAllGlobals(); });

/**
 * ⚠️ ADR 0113: the Keeper chooses for other people's machines, so the panel says
 * a required setting is locked everywhere before it offers to set one, and saves
 * only what members can apply.
 */
test("a Keeper requires a setting for every Hive, and the panel says what that means", async () => {
  const saved: unknown[] = [];
  vi.stubGlobal("fetch", vi.fn((_input: RequestInfo | URL, init?: RequestInit) => {
    const body = JSON.parse(String(init?.body)) as { settings: unknown[] };
    saved.push(body);
    return ok({ revision: saved.length + 1, settings: body.settings, supported });
  }));
  const onSaved = vi.fn();
  const policy: KeeperPolicy = { revision: 1, settings: [], supported };
  const view = render(<ApiaryPolicyPanel operatorToken="secret" policy={policy} onSaved={onSaved} />);

  expect(screen.getByText(/A required setting is locked on every Hive in this Apiary/)).toBeInTheDocument();
  expect(screen.getByText(/has to leave the Apiary/)).toBeInTheDocument();
  const select = screen.getByLabelText("Ordinary releases install automatically");
  expect(select).toHaveValue("");
  expect(screen.getByRole("button", { name: "Save policy" })).toBeDisabled();

  fireEvent.change(select, { target: { value: "on" } });
  fireEvent.click(screen.getByRole("button", { name: "Save policy" }));
  await waitFor(() => expect(saved).toEqual([{ settings: [{ key: "release.auto_install", value: "on" }] }]));
  await waitFor(() => expect(onSaved).toHaveBeenCalledWith(expect.objectContaining({ revision: 2 })));

  // The page hands back the saved revision; the editor starts from it.
  view.rerender(<ApiaryPolicyPanel operatorToken="secret" policy={onSaved.mock.calls[0][0] as KeeperPolicy} onSaved={onSaved} />);
  expect(screen.getByLabelText("Ordinary releases install automatically")).toHaveValue("on");
  expect(screen.getByRole("button", { name: "Save policy" })).toBeDisabled();

  // Back to each Hive deciding sends no setting at all, which lifts the lock.
  fireEvent.change(screen.getByLabelText("Ordinary releases install automatically"), { target: { value: "" } });
  fireEvent.click(screen.getByRole("button", { name: "Save policy" }));
  await waitFor(() => expect(saved[1]).toEqual({ settings: [] }));
});

test("an unsaved choice survives the page refreshing underneath it", () => {
  const policy: KeeperPolicy = { revision: 4, settings: [], supported };
  const view = render(<ApiaryPolicyPanel operatorToken="secret" policy={policy} onSaved={vi.fn()} />);
  fireEvent.change(screen.getByLabelText("Ordinary releases install automatically"), { target: { value: "off" } });
  view.rerender(<ApiaryPolicyPanel operatorToken="secret" policy={{ ...policy }} onSaved={vi.fn()} />);
  expect(screen.getByLabelText("Ordinary releases install automatically")).toHaveValue("off");
});

import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import type { KeeperPolicy } from "../api";
import ApiaryPolicyPanel from "./ApiaryPolicyPanel";
import { gapReason, settingValue } from "./policyLabels";

const ok = (body: unknown) => Promise.resolve(new Response(JSON.stringify(body), { headers: { "Content-Type": "application/json" } }));
const supported: KeeperPolicy["supported"] = [
  { key: "release.auto_install", kind: "choice", values: ["on", "off"] },
  { key: "shared_work.minimum_version", kind: "version", values: [] },
];

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

test("a minimum version for shared work is typed as a release number, and nothing else saves", async () => {
  const saved: unknown[] = [];
  vi.stubGlobal("fetch", vi.fn((_input: RequestInfo | URL, init?: RequestInit) => {
    const body = JSON.parse(String(init?.body)) as { settings: unknown[] };
    saved.push(body);
    return ok({ revision: 2, settings: body.settings, supported });
  }));
  const policy: KeeperPolicy = { revision: 1, settings: [{ key: "release.auto_install", value: "on" }], supported };
  render(<ApiaryPolicyPanel operatorToken="secret" policy={policy} onSaved={vi.fn()} />);

  const field = screen.getByLabelText("Minimum version for shared work");
  expect(field).toHaveValue("");
  expect(screen.getByText(/keeps its own work but can't take Apiary tasks/)).toBeInTheDocument();

  // A development build is no one's minimum, and the Keeper would refuse it.
  fireEvent.change(field, { target: { value: "1.16.9-dev-abc" } });
  expect(field).toHaveAttribute("aria-invalid", "true");
  expect(screen.getByText("Use a release number, such as 1.16.9.")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Save policy" })).toBeDisabled();

  fireEvent.change(field, { target: { value: " 1.16.10 " } });
  expect(field).toHaveAttribute("aria-invalid", "false");
  fireEvent.click(screen.getByRole("button", { name: "Save policy" }));
  await waitFor(() => expect(saved).toEqual([{ settings: [
    { key: "release.auto_install", value: "on" },
    { key: "shared_work.minimum_version", value: "1.16.10" },
  ] }]));
});

test("Queen autonomy is required tier by tier, in the words a member's own settings use", async () => {
  const saved: unknown[] = [];
  const withAutonomy: KeeperPolicy["supported"] = [
    ...supported,
    { key: "queen.autonomy.night_watch", kind: "choice", values: ["advisory", "coordinate", "local_execution"] },
  ];
  vi.stubGlobal("fetch", vi.fn((_input: RequestInfo | URL, init?: RequestInit) => {
    const body = JSON.parse(String(init?.body)) as { settings: unknown[] };
    saved.push(body);
    return ok({ revision: 2, settings: body.settings, supported: withAutonomy });
  }));
  render(<ApiaryPolicyPanel operatorToken="secret" policy={{ revision: 1, settings: [], supported: withAutonomy }} onSaved={vi.fn()} />);

  const nightWatch = screen.getByLabelText("Queen autonomy: Night Watch");
  expect(screen.getByRole("option", { name: "Advise only" })).toBeInTheDocument();
  fireEvent.change(nightWatch, { target: { value: "advisory" } });
  fireEvent.click(screen.getByRole("button", { name: "Save policy" }));
  await waitFor(() => expect(saved).toEqual([{ settings: [{ key: "queen.autonomy.night_watch", value: "advisory" }] }]));
});

test("Night Watch hours are two times in each Hive's own zone, saved only as a whole window", async () => {
  const saved: unknown[] = [];
  const withHours: KeeperPolicy["supported"] = [...supported, { key: "night_watch.hours", kind: "hours", values: [] }];
  vi.stubGlobal("fetch", vi.fn((_input: RequestInfo | URL, init?: RequestInit) => {
    const body = JSON.parse(String(init?.body)) as { settings: unknown[] };
    saved.push(body);
    return ok({ revision: 2, settings: body.settings, supported: withHours });
  }));
  render(<ApiaryPolicyPanel operatorToken="secret" policy={{ revision: 1, settings: [], supported: withHours }} onSaved={vi.fn()} />);

  const hours = screen.getByRole("group", { name: "Night Watch hours" });
  expect(hours).toHaveAccessibleDescription(/In each Hive's own time zone/);
  const [starts, ends] = [screen.getAllByLabelText("Starts")[0]!, screen.getAllByLabelText("Ends")[0]!];
  fireEvent.change(starts, { target: { value: "22:00" } });
  expect(screen.getByText("Choose both times, and make them different.")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Save policy" })).toBeDisabled();
  fireEvent.change(ends, { target: { value: "07:00" } });
  fireEvent.click(screen.getByRole("button", { name: "Save policy" }));
  await waitFor(() => expect(saved).toEqual([{ settings: [{ key: "night_watch.hours", value: "22:00-07:00" }] }]));

  expect(settingValue("night_watch.hours", "22:00-07:00")).toBe("22:00–07:00 in each Hive's own time");
  expect(gapReason({ key: "night_watch.hours", expected: "22:00-07:00", reason: "needs_local_choice" }))
    .toBe("This Hive's operator hasn't chosen its time zone for Night Watch yet");
});


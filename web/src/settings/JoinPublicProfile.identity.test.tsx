import { createRef } from "react";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import JoinPublicProfile, { type JoinPublicProfileHandle } from "./JoinPublicProfile";
import { fetchPublicHiveProfile, saveJoinPublicProfile } from "../api";
import { fetchEmailReadiness } from "../api/email";
import { fetchJiraReadiness } from "../api/jira";

vi.mock("../api", () => ({ fetchPublicHiveProfile: vi.fn(), saveJoinPublicProfile: vi.fn(), savePublicHiveProfile: vi.fn() }));
vi.mock("../api/email", () => ({ fetchEmailReadiness: vi.fn() }));
vi.mock("../api/jira", () => ({ fetchJiraReadiness: vi.fn() }));
afterEach(() => { cleanup(); vi.resetAllMocks(); });
const saved = { revision: 1, profile: { hive_name: "My Hive", operator_display_name: "Operator", contact_email: null as string | null } };
const connected = { configured: true, connection: "ready" as const, account_name: "Bea Bee", account_address: "bea@example.test" };
function open(profile = saved) {
  vi.mocked(fetchPublicHiveProfile).mockResolvedValue(profile);
  const ref = createRef<JoinPublicProfileHandle>();
  render(<JoinPublicProfile operatorToken="fictional" ref={ref} disabled={false} />);
  return ref;
}

test.each(["Jira", "Microsoft"])("%s supplies name and email without another form entry", async (provider) => {
  vi.mocked(fetchJiraReadiness).mockResolvedValue({ ...connected, connection: provider === "Jira" ? "ready" : "not_connected" });
  vi.mocked(fetchEmailReadiness).mockResolvedValue({ ...connected, connection: provider === "Microsoft" ? "ready" : "not_connected" });
  vi.mocked(saveJoinPublicProfile).mockResolvedValue({ ...saved, profile: { ...saved.profile, operator_display_name: "Bea Bee", contact_email: "bea@example.test" } });
  const ref = open();
  await waitFor(() => expect(screen.getByLabelText("Your name")).toHaveValue("Bea Bee"));
  expect(screen.getByLabelText("Contact email (optional)")).toHaveValue("bea@example.test");
  expect(screen.getByLabelText("Shared profile preview")).toHaveTextContent("Bea's Hive");
  expect(saveJoinPublicProfile).not.toHaveBeenCalled();
  await act(() => ref.current!.save());
  expect(saveJoinPublicProfile).toHaveBeenCalledWith("fictional", { hive_name: "My Hive", operator_display_name: "Bea Bee", contact_email: "bea@example.test" });
});

test("different connected identities require a choice instead of silently mixing details", async () => {
  vi.mocked(fetchJiraReadiness).mockResolvedValue(connected);
  vi.mocked(fetchEmailReadiness).mockResolvedValue({ ...connected, account_name: "Cora", account_address: "cora@example.test" });
  open();
  const choose = await screen.findByRole("button", { name: /Use Microsoft/ });
  expect(screen.getByLabelText("Your name")).toHaveValue("");
  expect(screen.getByLabelText("Contact email (optional)")).toHaveValue("");
  fireEvent.click(choose);
  expect(screen.getByLabelText("Your name")).toHaveValue("Cora");
  expect(screen.getByLabelText("Contact email (optional)")).toHaveValue("cora@example.test");
});

test("a saved complete profile is preserved without extra integration requests", async () => {
  open({ ...saved, profile: { ...saved.profile, operator_display_name: "Custom Bea", contact_email: "custom@example.test" } });
  expect(await screen.findByLabelText("Your name")).toHaveValue("Custom Bea");
  expect(fetchJiraReadiness).not.toHaveBeenCalled();
  expect(fetchEmailReadiness).not.toHaveBeenCalled();
});

test("late integration results cannot replace manual edits or cleared fields", async () => {
  let finish!: (value: typeof connected) => void;
  vi.mocked(fetchJiraReadiness).mockImplementation(() => new Promise((resolve) => { finish = resolve; }));
  vi.mocked(fetchEmailReadiness).mockRejectedValue(new Error("Offline"));
  open();
  fireEvent.change(await screen.findByLabelText("Your name"), { target: { value: "Custom Bea" } });
  fireEvent.change(screen.getByLabelText("Contact email (optional)"), { target: { value: "temp@example.test" } });
  fireEvent.change(screen.getByLabelText("Contact email (optional)"), { target: { value: "" } });
  await act(async () => finish(connected));
  expect(screen.getByLabelText("Your name")).toHaveValue("Custom Bea");
  expect(screen.getByLabelText("Contact email (optional)")).toHaveValue("");
});

test("unavailable integrations leave missing identity editable", async () => {
  vi.mocked(fetchJiraReadiness).mockRejectedValue(new Error("Offline"));
  vi.mocked(fetchEmailReadiness).mockRejectedValue(new Error("Offline"));
  open();
  expect(await screen.findByLabelText("Your name")).toBeEnabled();
  await act(async () => {});
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
});

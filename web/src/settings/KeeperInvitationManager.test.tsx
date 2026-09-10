import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import KeeperInvitationManager from "./KeeperInvitationManager";
import { approveApiaryJoinLink, createApiaryJoinLink, fetchApiaryJoinLinks } from "../api";

vi.mock("../api", async (importOriginal) => ({
  RuntimeRequestError: (await importOriginal<typeof import("../api")>()).RuntimeRequestError,
  fetchApiaryJoinLinks: vi.fn(),
  createApiaryJoinLink: vi.fn(),
  approveApiaryJoinLink: vi.fn(),
  revokeApiaryJoinLink: vi.fn(),
}));

afterEach(() => { cleanup(); vi.useRealTimers(); vi.restoreAllMocks(); vi.resetAllMocks(); vi.unstubAllGlobals(); });
const mount = () => render(<KeeperInvitationManager busy={false} operatorToken="fixture" onInvitationCreated={async () => undefined} />);
const flush = async () => { await act(async () => { await Promise.resolve(); }); };

test("a lost mutation response remains unconfirmed and is never automatically retried", async () => {
  vi.mocked(fetchApiaryJoinLinks).mockResolvedValue([]);
  vi.mocked(createApiaryJoinLink).mockRejectedValue(new TypeError("connection lost"));
  mount();
  await flush();
  fireEvent.click(screen.getByRole("button", { name: "Create invitation link" }));
  await flush();
  expect(screen.getByRole("alert")).toHaveTextContent("It may have been saved");
  expect(screen.getByRole("button", { name: "Create invitation link" })).toBeEnabled();
  expect(createApiaryJoinLink).toHaveBeenCalledTimes(1);
});

const bundle = {
  link: { id: "link-1", apiary_id: "apiary-1", apiary_name: "Fictional Garden",
    keeper_endpoint: "https://keeper.example.test", state: "open" as const, candidate: null,
    issued_at: 1, expires_at: 86401 },
  one_time_secret: "fictional",
};

test.each(["timeout", "unmount"] as const)("fences late invitation creation after %s without copying or retrying", async (reason) => {
  vi.useFakeTimers();
  const copy = vi.fn().mockResolvedValue(undefined);
  vi.stubGlobal("navigator", { clipboard: { writeText: copy } });
  vi.mocked(fetchApiaryJoinLinks).mockResolvedValue([]);
  let finish!: (value: typeof bundle) => void;
  let observed: AbortSignal | undefined;
  vi.mocked(createApiaryJoinLink).mockImplementation((_token, signal) => {
    observed = signal;
    return new Promise((resolve) => { finish = resolve; });
  });
  const view = mount();
  await flush();
  fireEvent.click(screen.getByRole("button", { name: "Create invitation link" }));
  await flush();
  expect(screen.getByRole("button", { name: "Working…" })).toBeDisabled();
  if (reason === "unmount") view.unmount();
  else {
    await act(async () => { await vi.advanceTimersByTimeAsync(8_000); });
    expect(screen.getByRole("button", { name: "Create invitation link" })).toBeEnabled();
    expect(screen.getByRole("alert")).toHaveTextContent("It may have been saved");
  }
  expect(observed?.aborted).toBe(true);
  await act(async () => { finish(bundle); });
  expect(screen.queryByRole("group", { name: "Created Apiary link" })).not.toBeInTheDocument();
  expect(copy).not.toHaveBeenCalled();
  expect(createApiaryJoinLink).toHaveBeenCalledTimes(1);
});

test("releases controls when a post-save refresh hangs without calling the saved change unconfirmed", async () => {
  vi.useFakeTimers();
  vi.mocked(fetchApiaryJoinLinks).mockResolvedValueOnce([]).mockImplementation(() => new Promise(() => undefined));
  vi.mocked(createApiaryJoinLink).mockResolvedValue(bundle);
  mount();
  await flush();
  fireEvent.click(screen.getByRole("button", { name: "Create invitation link" }));
  await flush();
  await act(async () => { await vi.advanceTimersByTimeAsync(8_000); });
  expect(screen.getByRole("button", { name: "Create invitation link" })).toBeEnabled();
  expect(screen.getByRole("alert")).toHaveTextContent("The invitation change was saved");
  expect(screen.getByRole("group", { name: "Created Apiary link" })).toBeVisible();
  expect(createApiaryJoinLink).toHaveBeenCalledTimes(1);
});

test("does not misreport a saved approval when refreshing invitation details fails", async () => {
  const link = { id: "link-1", apiary_id: "apiary-1", apiary_name: "Fictional Garden",
    keeper_endpoint: "https://keeper.example.test", state: "awaiting_approval" as const,
    candidate: { apiary_id: "apiary-1", card_issued_at: 1, card_expires_at: 86401,
      pinned_by_operator_id: "operator-1", pinned_at: 1, last_verified_at: 1,
      node_id: "node-2", hive_id: "hive-2", hive_name: "Clover",
      operator_id: "operator-2", operator_display_name: "Cora", public_key: "fictional" },
    issued_at: 1, expires_at: 86401 };
  vi.mocked(fetchApiaryJoinLinks).mockResolvedValueOnce([link]).mockResolvedValue([{ ...link, state: "approved" }]);
  vi.mocked(approveApiaryJoinLink).mockResolvedValue({ ...link, state: "approved" });
  render(<KeeperInvitationManager busy={false} operatorToken="fixture" onInvitationCreated={async () => { throw new Error("offline"); }} />);
  await flush();
  fireEvent.click(screen.getByRole("button", { name: "Approve Hive" }));
  await flush();
  expect(screen.getByRole("status")).toHaveTextContent("Clover is approved");
  expect(screen.getByRole("alert")).toHaveTextContent("Approval was saved");
  expect(screen.queryByRole("button", { name: "Approve Hive" })).not.toBeInTheDocument();
  expect(approveApiaryJoinLink).toHaveBeenCalledExactlyOnceWith("fixture", "link-1", expect.any(AbortSignal));
});

test("does not claim an empty invitation list before a successful read", async () => {
  vi.mocked(fetchApiaryJoinLinks).mockRejectedValue(new Error("offline"));
  mount();
  expect(screen.getByText("Checking invitation status…")).toBeVisible();
  await flush();
  expect(screen.getByText("Invitation status is unavailable.")).toBeVisible();
  expect(screen.queryByText(/No active invitation links/)).not.toBeInTheDocument();
  vi.mocked(fetchApiaryJoinLinks).mockResolvedValue([]);
  fireEvent.click(screen.getByRole("button", { name: "Check invitation status again" }));
  await flush();
  expect(screen.getByText(/No active invitation links/)).toBeVisible();
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
});

test("owns one read, aborts at the deadline and recovers on explicit retry", async () => {
  vi.useFakeTimers();
  let observed: AbortSignal | undefined;
  vi.mocked(fetchApiaryJoinLinks).mockImplementationOnce((_token, signal) => {
    observed = signal;
    return new Promise((_resolve, reject) => signal?.addEventListener("abort", () => reject(signal.reason)));
  }).mockResolvedValue([]);
  mount();
  await flush();
  await act(async () => { await vi.advanceTimersByTimeAsync(5_000); });
  expect(fetchApiaryJoinLinks).toHaveBeenCalledTimes(1);
  await act(async () => { await vi.advanceTimersByTimeAsync(3_000); });
  expect(observed?.aborted).toBe(true);
  expect(screen.getByRole("alert")).toHaveTextContent("could not be refreshed");
  fireEvent.click(screen.getByRole("button", { name: "Check invitation status again" }));
  await flush();
  expect(fetchApiaryJoinLinks).toHaveBeenCalledTimes(2);
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
});

test("does not poll hidden pages and aborts the active read on disposal", async () => {
  vi.useFakeTimers();
  const visibility = vi.spyOn(document, "visibilityState", "get").mockReturnValue("hidden");
  let signal: AbortSignal | undefined;
  vi.mocked(fetchApiaryJoinLinks).mockImplementation((_token, input) => {
    signal = input;
    return new Promise(() => undefined);
  });
  const view = mount();
  await act(async () => { await vi.advanceTimersByTimeAsync(15_000); });
  expect(fetchApiaryJoinLinks).not.toHaveBeenCalled();
  visibility.mockReturnValue("visible");
  fireEvent(document, new Event("visibilitychange"));
  await flush();
  expect(fetchApiaryJoinLinks).toHaveBeenCalledTimes(1);
  view.unmount();
  expect(signal?.aborted).toBe(true);
});

test("an older status read cannot erase a newly confirmed invitation", async () => {
  let resolveRead!: (value: Awaited<ReturnType<typeof fetchApiaryJoinLinks>>) => void;
  vi.mocked(fetchApiaryJoinLinks).mockImplementationOnce(() => new Promise((resolve) => { resolveRead = resolve; }));
  vi.mocked(createApiaryJoinLink).mockResolvedValue({
    link: { id: "link-1", apiary_id: "apiary-1", apiary_name: "Fictional Garden",
      keeper_endpoint: "https://keeper.example.test", state: "open", candidate: null,
      issued_at: 1, expires_at: 86401 },
    one_time_secret: "fictional",
  });
  mount();
  await flush();
  fireEvent.click(screen.getByRole("button", { name: "Create invitation link" }));
  await flush();
  await act(async () => { resolveRead([]); });
  expect(screen.getByRole("list", { name: "Apiary invitation links" })).toHaveTextContent("Waiting for Hive");
  expect(screen.queryByText(/No active invitation links/)).not.toBeInTheDocument();
  expect(createApiaryJoinLink).toHaveBeenCalledTimes(1);
});

test("confirmed creation remains successful when the following status read fails", async () => {
  vi.mocked(fetchApiaryJoinLinks).mockResolvedValueOnce([]).mockRejectedValue(new Error("offline"));
  vi.mocked(createApiaryJoinLink).mockResolvedValue({
    link: { id: "link-1", apiary_id: "apiary-1", apiary_name: "Fictional Garden",
      keeper_endpoint: "https://keeper.example.test", state: "open", candidate: null,
      issued_at: 1, expires_at: 86401 },
    one_time_secret: "fictional",
  });
  mount();
  await flush();
  fireEvent.click(screen.getByRole("button", { name: "Create invitation link" }));
  await flush();
  expect(screen.getByRole("status")).toHaveTextContent("Invitation link created");
  expect(screen.getByRole("alert")).toHaveTextContent("Showing the last confirmed information");
  expect(screen.getByRole("list", { name: "Apiary invitation links" })).toHaveTextContent("Waiting for Hive");
  expect(createApiaryJoinLink).toHaveBeenCalledTimes(1);
});

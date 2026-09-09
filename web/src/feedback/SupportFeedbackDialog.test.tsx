import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import SupportFeedbackDialog from "./SupportFeedbackDialog";
import { fetchSupportStatus, submitSupport, retrySupport, forgetSupportCopy, type SupportStatus } from "../api/support";
import { loadPendingSupport } from "./supportDraft";
import { RuntimeRequestError } from "../api/request";

vi.mock("../api/support", () => ({ fetchSupportStatus: vi.fn(), submitSupport: vi.fn(), retrySupport: vi.fn(), forgetSupportCopy: vi.fn() }));
const status: SupportStatus = { configured: true, sender: "running", deliveries: [] };
afterEach(() => { cleanup(); sessionStorage.clear(); vi.resetAllMocks(); });
function open() { return render(<SupportFeedbackDialog operatorToken="fixture" status={status} onClose={vi.fn()} />); }

test("Escape from discard confirmation keeps the original draft and parent dialog", () => {
  const onClose = vi.fn();
  render(<SupportFeedbackDialog operatorToken="fixture" status={status} onClose={onClose} />);
  const email = screen.getByLabelText("Email");
  email.focus();
  fireEvent.change(email, { target: { value: "fictional@example.invalid" } });
  fireEvent.keyDown(window, { key: "Escape" });
  expect(screen.getByRole("alertdialog", { name: "Discard this message?" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Keep editing" })).toHaveFocus();
  fireEvent.keyDown(window, { key: "Escape" });
  expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
  expect(email).toHaveValue("fictional@example.invalid");
  expect(email).toHaveFocus();
  expect(onClose).not.toHaveBeenCalled();
  expect(submitSupport).not.toHaveBeenCalled();
});

test("explains email replies without implying attachments or automatic diagnostics are sent", () => {
  open();
  expect(screen.getByText(/Swarm Support may reply by email/)).toHaveTextContent("Diagnostics are never uploaded automatically.");
  expect(screen.queryByText(/reply delivery are not enabled/)).not.toBeInTheDocument();
  expect(submitSupport).not.toHaveBeenCalled();
});

test("rate-limited reports show their retained deadline without an immediate retry", () => {
  render(<SupportFeedbackDialog operatorToken="fixture" onClose={vi.fn()} status={{ ...status, deliveries: [{
    submission_key: "fictional-key", created_at: 1,
    delivery: { state: "rate_limited", refusal: "rate_limited", retry_not_before: 120, attempts: 1, attempt_id: "attempt" },
  }] }} />);
  fireEvent.click(screen.getByText(/Delivery status ·/));
  expect(screen.getByText(/Support is busy — retry no earlier than/)).toBeVisible();
  expect(retrySupport).not.toHaveBeenCalled();
  expect(screen.queryByRole("button", { name: /retry/i })).not.toBeInTheDocument();
});

test("conflict details are visible without exposing a remote error body", () => {
  render(<SupportFeedbackDialog operatorToken="fixture" onClose={vi.fn()} status={{ ...status, deliveries: [{
    submission_key: "fictional-key", created_at: 1,
    delivery: { state: "failed", refusal: "conflict", attempts: 1, attempt_id: "attempt" },
  }] }} />);
  fireEvent.click(screen.getByText(/Delivery status ·/));
  expect(screen.getByText(/Support refused a conflicting report key/)).toBeVisible();
  expect(submitSupport).not.toHaveBeenCalled();
});
function review() {
  fireEvent.change(screen.getByLabelText("Email"), { target: { value: "fictional@example.invalid" } });
  fireEvent.change(screen.getByLabelText("Subject"), { target: { value: "Fictional report" } });
  fireEvent.change(screen.getByLabelText("Message"), { target: { value: "Exact reviewed words" } });
  fireEvent.click(screen.getByRole("button", { name: "Review message" }));
}

test("review does not send and uncertain reload retries the exact original report", async () => {
  vi.mocked(submitSupport).mockRejectedValueOnce(new Error("Lost response"));
  const first = open(); review();
  expect(submitSupport).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Send to Swarm Support" }));
  await screen.findByRole("alert");
  const original = vi.mocked(submitSupport).mock.calls[0][1];
  expect(loadPendingSupport()).toEqual(original);
  first.unmount(); open();
  expect(submitSupport).toHaveBeenCalledTimes(1);
  expect(screen.queryByRole("button", { name: "Edit message" })).toBeNull();
  vi.mocked(submitSupport).mockImplementationOnce(async (_token, input) => ({
    submission_key: input.submission_key, created_at: 1, delivery: { state: "pending", attempts: 0 },
  }));
  fireEvent.click(screen.getByRole("button", { name: "Retry this exact report" }));
  await screen.findByText("Saved to Hive — waiting to send");
  expect(vi.mocked(submitSupport).mock.calls[1][1]).toEqual(original);
  expect(loadPendingSupport()).toBeUndefined();
  expect(screen.queryByText("Received by Swarm Support")).toBeNull();
});

test("save does not imply received and explicit refresh reconciles confirmation", async () => {
  vi.mocked(submitSupport).mockImplementation(async (_token, input) => ({
    submission_key: input.submission_key, created_at: 1, delivery: { state: "pending", attempts: 0 },
  }));
  open(); review(); fireEvent.click(screen.getByRole("button", { name: "Send to Swarm Support" }));
  await screen.findByText("Saved to Hive — waiting to send");
  const key = vi.mocked(submitSupport).mock.calls[0][1].submission_key;
  vi.mocked(fetchSupportStatus).mockResolvedValue({ ...status, deliveries: [{ submission_key: key,
    created_at: 1, delivery: { state: "confirmed", attempts: 1 } }] });
  fireEvent.click(screen.getByText(/Delivery status ·/));
  fireEvent.click(screen.getByRole("button", { name: "Check delivery status" }));
  await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent("Received by Swarm Support"));
  expect(submitSupport).toHaveBeenCalledTimes(1);
});

test("a status-only matching key cannot discard an uncertain reviewed payload", async () => {
  vi.mocked(submitSupport).mockRejectedValueOnce(new Error("lost response"));
  open(); review(); fireEvent.click(screen.getByRole("button", { name: "Send to Swarm Support" }));
  await screen.findByRole("alert");
  const original = loadPendingSupport()!;
  vi.mocked(fetchSupportStatus).mockResolvedValue({ ...status, deliveries: [{ submission_key: original.submission_key,
    created_at: 1, delivery: { state: "confirmed", attempts: 1 } }] });
  fireEvent.click(screen.getByText(/Delivery status ·/));
  fireEvent.click(screen.getByRole("button", { name: "Check delivery status" }));
  await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("confirm its contents"));
  expect(screen.getByRole("button", { name: "Retry this exact report" })).toBeEnabled();
  expect(loadPendingSupport()).toEqual(original);
});

test("failure to retain a safe retry copy prevents sending", async () => {
  const set = vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => { throw new Error("Full"); });
  open(); review(); fireEvent.click(screen.getByRole("button", { name: "Send to Swarm Support" }));
  await screen.findByRole("alert");
  expect(submitSupport).not.toHaveBeenCalled();
  set.mockRestore();
});

test("manual retry lost response survives reload without granting a second retry", async () => {
  const exhausted: SupportStatus = { ...status, deliveries: [{
    submission_key: "00000000-0000-0000-0000-000000000007", created_at: 1,
    delivery: { state: "uncertain", attempts: 5, attempt_id: "00000000-0000-0000-0000-000000000008" },
  }] };
  const mount = () => render(<SupportFeedbackDialog operatorToken="fixture" status={exhausted} onClose={vi.fn()} />);
  vi.mocked(retrySupport).mockRejectedValueOnce(new Error("lost response"));
  const first = mount(); fireEvent.click(screen.getByText(/Delivery status ·/));
  fireEvent.click(screen.getByRole("button", { name: "Retry once" }));
  await screen.findByRole("alert");
  const command = vi.mocked(retrySupport).mock.calls[0][1];
  first.unmount(); mount();
  expect(retrySupport).toHaveBeenCalledTimes(1);
  vi.mocked(retrySupport).mockResolvedValueOnce({ state: "uncertain", attempts: 5, manual_retry_pending: true });
  fireEvent.click(screen.getByText(/Delivery status ·/));
  fireEvent.click(screen.getByRole("button", { name: "Retry once" }));
  await screen.findByText(/One retry requested/);
  expect(vi.mocked(retrySupport).mock.calls[1][1]).toEqual(command);
});

test("definitively stale retry clears only its command, not the saved report", async () => {
  const exhausted: SupportStatus = { ...status, deliveries: [{
    submission_key: "00000000-0000-0000-0000-000000000007", created_at: 1,
    delivery: { state: "uncertain", attempts: 5, attempt_id: "00000000-0000-0000-0000-000000000008" },
  }] };
  vi.mocked(retrySupport).mockRejectedValue(new RuntimeRequestError(409, "changed"));
  render(<SupportFeedbackDialog operatorToken="fixture" status={exhausted} onClose={vi.fn()} />);
  fireEvent.click(screen.getByText(/Delivery status ·/));
  fireEvent.click(screen.getByRole("button", { name: "Retry once" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Check delivery status");
  expect(sessionStorage.getItem("swarm.support.retry.v1")).toBeNull();
  expect(screen.getByText(/Delivery unconfirmed/)).toBeInTheDocument();
});

test("only confirmed copies offer explicit local removal, including when support is disabled", async () => {
  const rows: SupportStatus = { ...status, configured: false, deliveries: [
    { submission_key: "confirmed", created_at: 1, delivery: { state: "confirmed", attempts: 1, receipt: { message_id: "central-message" } } },
    { submission_key: "uncertain", created_at: 2, delivery: { state: "uncertain", attempts: 5 } },
  ] };
  vi.mocked(forgetSupportCopy).mockResolvedValue();
  render(<SupportFeedbackDialog operatorToken="fixture" status={rows} onClose={vi.fn()} />);
  fireEvent.click(screen.getByText(/Delivery status ·/));
  expect(screen.getAllByRole("button", { name: "Remove from this Hive…" })).toHaveLength(1);
  fireEvent.click(screen.getByRole("button", { name: "Remove from this Hive…" }));
  expect(forgetSupportCopy).not.toHaveBeenCalled();
  expect(screen.getByText(/conversation and history remain in Swarm Support/)).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Remove local copy" }));
  await waitFor(() => expect(screen.queryByRole("button", { name: "Remove local copy" })).toBeNull());
  expect(forgetSupportCopy).toHaveBeenCalledTimes(1);
  expect(screen.getByText(/Delivery unconfirmed/)).toBeInTheDocument();
});

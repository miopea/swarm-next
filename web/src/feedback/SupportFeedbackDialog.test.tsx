import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import SupportFeedbackDialog from "./SupportFeedbackDialog";
import { fetchSupportStatus, submitSupport, type SupportStatus } from "../api/support";
import { loadPendingSupport } from "./supportDraft";

vi.mock("../api/support", () => ({ fetchSupportStatus: vi.fn(), submitSupport: vi.fn() }));
const status: SupportStatus = { configured: true, sender: "running", deliveries: [] };
afterEach(() => { cleanup(); sessionStorage.clear(); vi.resetAllMocks(); });
function open() { return render(<SupportFeedbackDialog operatorToken="fixture" status={status} onClose={vi.fn()} />); }
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

test("failure to retain a safe retry copy prevents sending", async () => {
  const set = vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => { throw new Error("Full"); });
  open(); review(); fireEvent.click(screen.getByRole("button", { name: "Send to Swarm Support" }));
  await screen.findByRole("alert");
  expect(submitSupport).not.toHaveBeenCalled();
  set.mockRestore();
});

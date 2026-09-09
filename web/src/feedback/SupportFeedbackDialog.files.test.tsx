import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { IDBFactory } from "fake-indexeddb";
import SupportFeedbackDialog from "./SupportFeedbackDialog";
import { submitSupportFiles, submitSupport, type SupportStatus } from "../api/support";
import * as storage from "./supportFiles";

vi.mock("../api/support", () => ({ fetchSupportStatus: vi.fn(), submitSupportFiles: vi.fn(), submitSupport: vi.fn(), retrySupport: vi.fn(), forgetSupportCopy: vi.fn() }));
const status: SupportStatus = { configured: true, attachments_supported: true, sender: "running", deliveries: [] };
beforeEach(async () => {
  vi.stubGlobal("indexedDB", new IDBFactory());
  vi.stubGlobal("crypto", (await vi.importActual<{ webcrypto: Crypto }>("node:crypto")).webcrypto);
});
afterEach(() => { cleanup(); sessionStorage.clear(); vi.restoreAllMocks(); vi.resetAllMocks(); vi.unstubAllGlobals(); });
async function open() {
  const view = render(<SupportFeedbackDialog operatorToken="fixture" status={status} onClose={vi.fn()} />);
  await waitFor(() => expect(screen.queryByText("Checking for a saved attachment report…")).not.toBeInTheDocument());
  return view;
}
async function prepare() {
  fireEvent.change(screen.getByLabelText("Email"), { target: { value: "fictional@example.invalid" } });
  fireEvent.change(screen.getByLabelText("Subject"), { target: { value: "Fictional file report" } });
  fireEvent.change(screen.getByLabelText("Message"), { target: { value: "Reviewed words" } });
  const file = new File(["fictional bytes"], "fictional.txt", { type: "text/plain" });
  Object.defineProperty(file, "arrayBuffer", { value: async () => new TextEncoder().encode("fictional bytes").buffer });
  fireEvent.change(screen.getByLabelText(/Attachments \(optional\)/), { target: { files: [file] } });
  await screen.findByRole("button", { name: "Remove fictional.txt" });
  fireEvent.click(screen.getByRole("button", { name: "Review message" }));
  expect(screen.getByRole("region", { name: "Review support message" })).toHaveTextContent("fictional.txt");
}

test("reviewed files are retained across uncertain reload and only explicitly retried", async () => {
  vi.mocked(submitSupportFiles).mockRejectedValueOnce(new Error("lost response"));
  const first = await open(); await prepare();
  expect(submitSupportFiles).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Send to Swarm Support" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("save could not be confirmed");
  const original = vi.mocked(submitSupportFiles).mock.calls[0][1];
  expect(await storage.loadPendingSupportFiles()).toEqual(original);
  first.unmount(); await open();
  expect(submitSupportFiles).toHaveBeenCalledTimes(1);
  expect(screen.queryByRole("button", { name: "Edit message" })).not.toBeInTheDocument();
  expect(screen.getByRole("region", { name: "Review support message" })).toHaveTextContent("fictional.txt");
  vi.mocked(submitSupportFiles).mockImplementationOnce(async (_token, report) => ({
    submission_key: report.submission.submission_key, created_at: 1, delivery: { state: "pending", attempts: 0 },
  }));
  fireEvent.click(screen.getByRole("button", { name: "Retry this exact report" }));
  await screen.findByText("Saved to Hive — waiting to send");
  expect(vi.mocked(submitSupportFiles).mock.calls[1][1]).toEqual(original);
  await waitFor(async () => expect(await storage.loadPendingSupportFiles()).toBeUndefined());
  expect(submitSupport).not.toHaveBeenCalled();
});

test("failed browser retention prevents the upload and leaves the reviewed files editable", async () => {
  await open(); await prepare();
  vi.spyOn(storage, "savePendingSupportFiles").mockRejectedValue(new Error("Browser storage is full. Nothing was sent."));
  fireEvent.click(screen.getByRole("button", { name: "Send to Swarm Support" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Browser storage is full");
  expect(submitSupportFiles).not.toHaveBeenCalled();
  expect(screen.getByRole("button", { name: "Edit message" })).toBeEnabled();
});

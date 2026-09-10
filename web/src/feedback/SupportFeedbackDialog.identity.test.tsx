import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import SupportFeedbackDialog from "./SupportFeedbackDialog";
import { fetchPublicHiveProfile } from "../api";
import { submitSupport } from "../api/support";
import { loadPendingSupport, savePendingSupport } from "./supportDraft";

vi.mock("../api", () => ({ fetchPublicHiveProfile: vi.fn() }));
vi.mock("../api/support", () => ({ submitSupport: vi.fn() }));
afterEach(() => { cleanup(); sessionStorage.clear(); vi.resetAllMocks(); });
const profile = { revision: 2, profile: { hive_name: "Clover", operator_display_name: "Bea Bee", contact_email: "bea@example.test" } };
function open() {
  const onClose = vi.fn();
  render(<SupportFeedbackDialog operatorToken="fictional" status={{ configured: true, sender: "running", deliveries: [] }} onClose={onClose} />);
  return onClose;
}

test("saved identity prefills feedback without sending or creating a dirty draft", async () => {
  vi.mocked(fetchPublicHiveProfile).mockResolvedValue(profile);
  const close = open();
  await waitFor(() => expect(screen.getByLabelText("Email")).toHaveValue("bea@example.test"));
  expect(screen.getByLabelText("Name (optional)")).toHaveValue("Bea Bee");
  fireEvent.click(screen.getByRole("button", { name: "Close" }));
  expect(close).toHaveBeenCalledOnce();
  expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
  expect(submitSupport).not.toHaveBeenCalled();
});

test("a late profile cannot overwrite edits, including an intentionally cleared field", async () => {
  let finish!: (value: typeof profile) => void;
  vi.mocked(fetchPublicHiveProfile).mockImplementation(() => new Promise((resolve) => { finish = resolve; }));
  open();
  fireEvent.change(screen.getByLabelText("Email"), { target: { value: "other@example.test" } });
  fireEvent.change(screen.getByLabelText("Name (optional)"), { target: { value: "Temporary" } });
  fireEvent.change(screen.getByLabelText("Name (optional)"), { target: { value: "" } });
  await act(async () => finish(profile));
  expect(screen.getByLabelText("Email")).toHaveValue("other@example.test");
  expect(screen.getByLabelText("Name (optional)")).toHaveValue("");
});

test("missing identity remains editable and never fills the placeholder Operator", async () => {
  vi.mocked(fetchPublicHiveProfile).mockResolvedValue({ revision: 1, profile: { ...profile.profile, operator_display_name: "Operator", contact_email: null } });
  open();
  await act(async () => {});
  expect(screen.getByLabelText("Name (optional)")).toHaveValue("");
  expect(screen.getByLabelText("Email")).toHaveValue("");
});

test("failed profile lookup does not block the feedback form", async () => {
  vi.mocked(fetchPublicHiveProfile).mockRejectedValue(new Error("Offline"));
  open();
  await act(async () => {});
  expect(screen.getByLabelText("Email")).toBeEnabled();
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
});

test("loading a new profile never changes a retained report awaiting retry", async () => {
  const retained = { submission_key: "00000000-0000-4000-8000-000000000001", kind: "bug_report" as const,
    name: "Original Sender", email: "original@example.test", subject: "Fictional report", body: "Keep this exact report." };
  savePendingSupport(retained);
  vi.mocked(fetchPublicHiveProfile).mockResolvedValue(profile);
  open();
  await act(async () => {});
  expect(screen.getByRole("region", { name: "Review support message" })).toHaveTextContent("Original Sender · original@example.test");
  expect(loadPendingSupport()).toEqual(retained);
  expect(submitSupport).not.toHaveBeenCalled();
});

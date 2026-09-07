import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import FeedbackDialog from "./FeedbackDialog";
import { fetchSupportStatus } from "../api/support";
import type { ComponentProps } from "react";

vi.mock("../api/support", () => ({ fetchSupportStatus: vi.fn() }));
vi.mock("./DogfoodFeedbackDialog", () => ({ default: () => <p>Existing diagnostic workflow</p> }));
vi.mock("./SupportFeedbackDialog", () => ({ default: () => <p>Private support workflow</p> }));
afterEach(() => { cleanup(); sessionStorage.clear(); vi.resetAllMocks(); });
const props = { operatorToken: "fixture", onClose: vi.fn() } as unknown as ComponentProps<typeof FeedbackDialog>;

test("failed destination lookup never selects the existing public workflow", async () => {
  vi.mocked(fetchSupportStatus).mockRejectedValue(new Error("offline"));
  render(<FeedbackDialog {...props} />);
  expect(await screen.findByRole("alert")).toHaveTextContent("Nothing has been sent");
  expect(screen.queryByText("Existing diagnostic workflow")).toBeNull();
});

test.each([true, false])("known configured=%s selects only the matching workflow", async (configured) => {
  vi.mocked(fetchSupportStatus).mockResolvedValue({ configured, sender: null, deliveries: [] });
  render(<FeedbackDialog {...props} />);
  await screen.findByText(configured ? "Private support workflow" : "Existing diagnostic workflow");
});

test("disabled configuration does not hide existing central deliveries", async () => {
  vi.mocked(fetchSupportStatus).mockResolvedValue({ configured: false, sender: null, deliveries: [{
    submission_key: "saved-report", created_at: 1, delivery: { state: "uncertain", attempts: 1 },
  }] });
  render(<FeedbackDialog {...props} />);
  await screen.findByText("Private support workflow");
});

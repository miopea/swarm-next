import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import WorkerStartNotice from "./WorkerStartNotice";

afterEach(cleanup);
const hold = { kind: "wake_not_admitted", subject: "wake:fictional-one", worker_name: "Orchard API", reason: "Orchard API waits for memory pressure to ease.", first_observed_at: 1, observations: 2 };

test("a known start safeguard is compact, deduplicates reasons and clears on recovery", () => {
  const diagnostics = vi.fn();
  const view = render(<WorkerStartNotice held={[hold, { ...hold, subject: "wake:fictional-two" }]} unavailable={false} onDiagnostics={diagnostics} />);
  const notice = screen.getByLabelText("Worker start safeguards");
  expect(notice).not.toHaveAttribute("open");
  fireEvent.click(screen.getByText("Worker starts paused · 2 items"));
  expect(screen.getAllByText(hold.reason)).toHaveLength(1);
  expect(screen.getByText(/not a request for approval/)).toBeVisible();
  fireEvent.click(screen.getByRole("button", { name: "Check diagnostics" }));
  expect(diagnostics).toHaveBeenCalledOnce();
  view.rerender(<WorkerStartNotice held={[]} unavailable={false} onDiagnostics={diagnostics} />);
  expect(screen.queryByLabelText("Worker start safeguards")).toBeNull();
});

test("failed observation preserves the hold but does not claim fresh evidence", () => {
  const view = render(<WorkerStartNotice held={[hold]} unavailable onDiagnostics={() => {}} />);
  fireEvent.click(screen.getByText("Worker starts paused · 1 item"));
  expect(screen.getByRole("status")).toHaveTextContent("last recorded holds");
  view.rerender(<WorkerStartNotice held={[hold]} unavailable={false} onDiagnostics={() => {}} />);
  expect(screen.queryByRole("status")).toBeNull();
  expect(screen.getByText(hold.reason)).toBeVisible();
});

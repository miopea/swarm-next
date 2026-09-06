import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import StaleBundleNotice from "./StaleBundleNotice";

afterEach(cleanup);
test("a version mismatch waits for an explicit reload and explains unsent-form risk", () => {
  const reload = vi.fn();
  render(<StaleBundleNotice stale serverVersion="new" dismissed={null} onDismiss={vi.fn()} onReload={reload} />);
  expect(screen.getByText(/Finish or save any unsent forms/)).toBeVisible();
  expect(reload).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Reload this tab" }));
  expect(reload).toHaveBeenCalledExactlyOnceWith("new");
});
test("dismissal applies only to the observed version and recovered mismatch disappears", () => {
  const dismiss = vi.fn();
  const props = { stale: true, serverVersion: "one", dismissed: null as string | null, onDismiss: dismiss };
  const view = render(<StaleBundleNotice {...props} />);
  fireEvent.click(screen.getByRole("button", { name: "Not now" }));
  expect(dismiss).toHaveBeenCalledExactlyOnceWith("one");
  view.rerender(<StaleBundleNotice {...props} dismissed="one" />);
  expect(screen.queryByRole("status")).not.toBeInTheDocument();
  view.rerender(<StaleBundleNotice {...props} dismissed="one" serverVersion="two" />);
  expect(screen.getByRole("status")).toBeVisible();
  view.rerender(<StaleBundleNotice {...props} stale={false} />);
  expect(screen.queryByRole("status")).not.toBeInTheDocument();
});

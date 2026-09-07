import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import StaleBundleNotice, { reloadBrowser } from "./StaleBundleNotice";

afterEach(cleanup);

test("updating the bundle stamps the URL before a real reload without adding history", () => {
  const original = window.location.href;
  const originalState = window.history.state;
  try {
    window.history.replaceState({ fixture: true }, "", "/?v=old&surface=queues#section");
    const length = window.history.length;
    const reload = vi.fn(() => {
      expect(new URL(window.location.href).searchParams.get("v")).toBe("new");
      expect(new URL(window.location.href).searchParams.get("surface")).toBe("queues");
      expect(window.location.hash).toBe("#section");
      expect(window.history.state).toEqual({ fixture: true });
      expect(window.history.length).toBe(length);
    });
    reloadBrowser("new", reload);
    expect(reload).toHaveBeenCalledExactlyOnceWith();
  } finally {
    window.history.replaceState(originalState, "", original);
  }
});
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

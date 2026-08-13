import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import CommandPalette from "./CommandPalette";

afterEach(cleanup);

test("filters and opens a named worker", () => {
  const run = vi.fn();
  const close = vi.fn();
  render(<CommandPalette onClose={close} choices={[
    { id: "tasks", label: "Tasks", detail: "Plan and dispatch", group: "Go to", run: vi.fn() },
    { id: "daisy", label: "Daisy", detail: "Open worker terminal", group: "Workers", run },
  ]} />);
  fireEvent.change(screen.getByLabelText("Find a view or worker"), { target: { value: "dai" } });
  expect(screen.queryByRole("button", { name: /Tasks/ })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: /Daisy/ }));
  expect(close).toHaveBeenCalledOnce();
  expect(run).toHaveBeenCalledOnce();
});

test("offers a visible close action for touch users", () => {
  const close = vi.fn();
  render(<CommandPalette onClose={close} choices={[]} />);
  fireEvent.click(screen.getByRole("button", { name: "Close" }));
  expect(close).toHaveBeenCalledOnce();
});

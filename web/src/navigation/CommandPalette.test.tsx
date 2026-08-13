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
  fireEvent.change(screen.getByLabelText("Find work, decisions, or workers"), { target: { value: "dai" } });
  expect(screen.queryByRole("button", { name: /Tasks/ })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("option", { name: /Daisy/ }));
  expect(close).toHaveBeenCalledOnce();
  expect(run).toHaveBeenCalledOnce();
});

test("offers a visible close action for touch users", () => {
  const close = vi.fn();
  render(<CommandPalette onClose={close} choices={[]} />);
  fireEvent.click(screen.getByRole("button", { name: "Close" }));
  expect(close).toHaveBeenCalledOnce();
});

test("makes the wake action explicit for sleeping workers", () => {
  const wake = vi.fn();
  render(<CommandPalette onClose={vi.fn()} choices={[
    { id: "daisy", label: "Daisy", detail: "Wake sleeping worker", group: "Workers", run: wake },
  ]} />);
  expect(screen.getByRole("option", { name: /Daisy Wake sleeping worker/ })).toBeInTheDocument();
  expect(screen.getByText(/Sleeping workers wake when selected/)).toBeInTheDocument();
});

test("searches open work and attention alongside workers", () => {
  const openWork = vi.fn();
  render(<CommandPalette onClose={vi.fn()} choices={[
    { id: "task", label: "Repair mobile layout", detail: "In progress · Daisy", group: "Work", run: openWork },
    { id: "decision", label: "Approve production", detail: "Worker needs deployment authority", group: "Attention", run: vi.fn() },
  ]} />);
  fireEvent.change(screen.getByLabelText("Find work, decisions, or workers"), { target: { value: "mobile" } });
  expect(screen.getByRole("option", { name: /Work Repair mobile layout In progress · Daisy/ })).toBeInTheDocument();
  expect(screen.queryByRole("option", { name: /Approve production/ })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("option", { name: /Repair mobile layout/ }));
  expect(openWork).toHaveBeenCalledOnce();
});

test("opens the selected result entirely from the keyboard", () => {
  const first = vi.fn();
  const second = vi.fn();
  const close = vi.fn();
  render(<CommandPalette onClose={close} choices={[
    { id: "queen", label: "Queen", detail: "Open worker terminal", group: "Workers", run: first },
    { id: "task", label: "Repair mobile layout", detail: "In progress", group: "Work", run: second },
  ]} />);
  const search = screen.getByRole("combobox", { name: "Find work, decisions, or workers" });

  fireEvent.keyDown(search, { key: "ArrowDown" });
  expect(screen.getByRole("option", { name: /Repair mobile layout/ })).toHaveAttribute("aria-selected", "true");
  fireEvent.keyDown(search, { key: "Enter" });

  expect(first).not.toHaveBeenCalled();
  expect(second).toHaveBeenCalledOnce();
  expect(close).toHaveBeenCalledOnce();
});

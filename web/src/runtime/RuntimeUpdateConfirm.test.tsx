import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import RuntimeUpdateConfirm from "./RuntimeUpdateConfirm";
import type { RuntimeUpdateSummary } from "./runtimeUpdates";

afterEach(cleanup);

const release: RuntimeUpdateSummary = {
  kind: "app",
  label: "App and API update",
  detail: "Revision 5394d9a is ready to build. Workers stay online through an App and API release.",
  busy: false,
  action: "build",
  actionLabel: "Build and release",
};

const engine: RuntimeUpdateSummary = {
  kind: "worker_engine",
  label: "Worker engine update",
  detail: "A worker engine update is installed but not running. Applying it restarts loaded workers.",
  busy: false,
  action: "apply_worker_engine",
  actionLabel: "Apply worker engine update",
  consequence: "Every loaded worker is stopped and brought back.",
};

test("an update that stops running work says so before offering the button", () => {
  // The operator asked for "more of a warning for destructive actions (like
  // worker engine or agent updates)". The weight comes from the consequence
  // rather than from the kind, so a new update that takes workers away gets the
  // same treatment without anyone remembering to add it to a list.
  const onConfirm = vi.fn();
  render(<RuntimeUpdateConfirm update={engine} busy={false} onConfirm={onConfirm} onCancel={vi.fn()} />);

  expect(screen.getByRole("alertdialog")).toHaveClass("destructive");
  expect(screen.getByText("This stops running work")).toBeInTheDocument();
  expect(screen.getByRole("alert")).toHaveTextContent("Every loaded worker is stopped");
  expect(screen.getByRole("button", { name: "Apply worker engine update" })).toHaveClass("destructive-action");

  fireEvent.click(screen.getByRole("button", { name: "Apply worker engine update" }));
  expect(onConfirm).toHaveBeenCalledOnce();
});

test("an update that keeps workers online is not dressed as dangerous", () => {
  // Warning about everything is the same as warning about nothing. An App and
  // API release does not interrupt a worker, and must not read as though it
  // does, or the worker engine warning stops meaning anything.
  render(<RuntimeUpdateConfirm update={release} busy={false} onConfirm={vi.fn()} onCancel={vi.fn()} />);

  expect(screen.getByRole("alertdialog")).not.toHaveClass("destructive");
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Build and release" })).toHaveClass("primary-action");
});

test("cancelling runs nothing", () => {
  const onConfirm = vi.fn();
  const onCancel = vi.fn();
  render(<RuntimeUpdateConfirm update={engine} busy={false} onConfirm={onConfirm} onCancel={onCancel} />);

  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));

  expect(onCancel).toHaveBeenCalledOnce();
  expect(onConfirm).not.toHaveBeenCalled();
});

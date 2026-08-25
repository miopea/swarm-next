import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import type { Task, Worker } from "../api";
import WorkerContextBar from "./WorkerContextBar";

afterEach(cleanup);

const worker: Worker = {
  id: "worker-1", hive_id: "hive", name: "Public Website", role: "worker", provider: "claude_code",
  workspace: "/home/operator/projects/rcg/rcg-public-web", autostart: false, position: 1,
  active_session_id: "session", running: true, attention_state: "resting", created_at: 1, updated_at: 1,
};

const task: Task = {
  id: "task-1", hive_id: "hive", title: "Render content blocks", description: "", operator_instruction: "", priority: "normal",
  workspace: "/repo", state: "active", assigned_worker_id: "worker-1", assigned_session_id: null,
  position: 1, created_at: 1, updated_at: 1,
};

const render_ = (over: Partial<React.ComponentProps<typeof WorkerContextBar>> = {}) =>
  render(
    <WorkerContextBar
      worker={worker}
      currentTask={task}
      openCount={1}
      taskStateLabel={() => "In progress"}
      onOpenQueue={vi.fn()}
      {...over}
    />,
  );

test("opens the worker's queue focused on the task it names", () => {
  const onOpenQueue = vi.fn();
  render_({ onOpenQueue });

  fireEvent.click(screen.getByRole("button", { name: /Render content blocks/ }));

  // Both arguments matter: a focused task behind a filter that hides it is why
  // a one-task worker showed an empty board.
  expect(onOpenQueue).toHaveBeenCalledWith("worker-1", "task-1");
});

test("the workload summary says whose work it counts", () => {
  // Read beside a single task row, a bare "1 active · 5 blocked · 1 ready"
  // invites the reading that those numbers describe THIS task. The operator
  // asked what it meant.
  render_({ openCount: 7, workSummary: "1 active · 5 blocked · 1 ready" });

  expect(screen.getByRole("button", { name: /Show all/ })).toHaveAttribute(
    "title",
    "Public Website's open work: 1 active · 5 blocked · 1 ready",
  );
});

test("offers the rest of the queue only when there is more than one task", () => {
  render_({ openCount: 1 });
  expect(screen.queryByRole("button", { name: /Show all/ })).not.toBeInTheDocument();

  cleanup();
  const onOpenQueue = vi.fn();
  render_({ openCount: 4, onOpenQueue });
  const queue = screen.getByRole("button", { name: "Show all 4 open tasks for Public Website" });

  // Three *others*, beside the one already named.
  expect(queue).toHaveTextContent("+3");
  fireEvent.click(queue);
  expect(onOpenQueue).toHaveBeenCalledWith("worker-1");
});

test("says the branch matches HEAD when nothing differs", () => {
  render_({ repository: { branch: "main", detached: false, changed_paths: 0 } });

  expect(screen.getByText("main").parentElement).toHaveAttribute(
    "title",
    "rcg-public-web on main, matching HEAD",
  );
});

test("counts the paths that differ from HEAD", () => {
  render_({ repository: { branch: "main", detached: false, changed_paths: 3 } });

  expect(screen.getByText("3")).toBeInTheDocument();
  expect(screen.getByText("main").parentElement).toHaveAttribute(
    "title",
    "rcg-public-web on main, with 3 path(s) differing from HEAD",
  );
});

test("names a detached HEAD rather than inventing a branch", () => {
  render_({ repository: { branch: null, detached: true, changed_paths: 0 } });

  expect(screen.getByText("detached").parentElement).toHaveAttribute(
    "title",
    "rcg-public-web has a detached HEAD",
  );
});

test("says nothing about a workspace that is not a checkout", () => {
  render_({ repository: null });

  expect(screen.queryByText("detached")).not.toBeInTheDocument();
  expect(document.querySelector(".worker-repository")).toBeNull();
});

test("names the device driving the worker, when it is not this one", () => {
  render_({ engagement: { deviceClass: "mobile", detail: "a phone is driving this worker" } });

  expect(screen.getByRole("status")).toHaveTextContent("On phone");
});

test("says what the unconfirmed-delivery mark means and where to look", () => {
  // The roster mark explains itself on hover and then leads nowhere: opening
  // the worker said nothing about what it meant or how to clear it.
  render_({ unconfirmedDelivery: true });

  expect(screen.getByRole("status")).toHaveTextContent("Briefing unconfirmed");
  expect(screen.getByRole("status")).toHaveTextContent("check the terminal");
});

test("stays quiet when every briefing was confirmed", () => {
  render_({ unconfirmedDelivery: false });

  expect(screen.queryByText(/Briefing unconfirmed/)).not.toBeInTheDocument();
});

test("offers to take a worker held by another screen, without typing into it", () => {
  // ADR 0049. The badge named the device holding the worker and offered nothing
  // to do about it, so the only way to take it back was to type into the worker
  // — which sends real input to a real provider. Reclaiming a screen and
  // instructing an agent are not the same act.
  const onClaim = vi.fn();
  render(
    <WorkerContextBar
      worker={worker}
      currentTask={task}
      openCount={1}
      engagement={{ deviceClass: "desktop", detail: "Held by another desktop" }}
      taskStateLabel={() => "Active"}
      onOpenQueue={vi.fn()}
      onClaim={onClaim}
    />,
  );

  expect(screen.getByText("On another desktop")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Work here" }));
  expect(onClaim).toHaveBeenCalledOnce();
});

test("says nothing about claiming a worker no other screen holds", () => {
  render(
    <WorkerContextBar
      worker={worker}
      currentTask={task}
      openCount={1}
      taskStateLabel={() => "Active"}
      onOpenQueue={vi.fn()}
      onClaim={vi.fn()}
    />,
  );

  expect(screen.queryByRole("button", { name: "Work here" })).not.toBeInTheDocument();
});

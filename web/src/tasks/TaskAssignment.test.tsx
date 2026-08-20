import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import type { Task, Worker } from "../api";
import TaskAssignment from "./TaskAssignment";

afterEach(cleanup);

const worker = (over: Partial<Worker> = {}): Worker => ({
  id: "worker-1", hive_id: "hive-1", name: "Sculpt Studio", role: "worker", provider: "claude_code",
  workspace: "/workspace/sculpt", autostart: false, position: 1, active_session_id: "session-new",
  running: true, attention_state: "buzzing", created_at: 1, updated_at: 1, ...over,
});

const task = (over: Partial<Task> = {}): Task => ({
  id: "task-1", hive_id: "hive-1", title: "Ship the reader", description: "", operator_instruction: "",
  priority: "normal", workspace: "/workspace/sculpt", state: "ready", assigned_worker_id: "worker-1",
  // Recorded before the worker restarted, so it names a session that is gone.
  assigned_session_id: "session-that-ended", position: 1, created_at: 1, updated_at: 1, ...over,
});

const render_ = (t: Task, workers: Worker[]) => render(
  <TaskAssignment
    task={t}
    workers={workers}
    busy={false}
    onAssign={vi.fn()}
    onOpenWorker={vi.fn()}
    onTransition={vi.fn()}
    onStartWorker={vi.fn()}
  />,
);

test("does not offer to wake a worker that is already running", () => {
  // Every worker gets a new session when it restarts. Judging running-ness from
  // the session the task recorded made every task assigned before a restart
  // offer to wake a worker that was working at that moment.
  render_(task(), [worker()]);

  expect(screen.queryByRole("button", { name: /^Wake/ })).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Start work" })).toBeInTheDocument();
});

test("still offers to wake a worker that is genuinely asleep", () => {
  render_(task(), [worker({ running: false, active_session_id: null, attention_state: "sleeping" })]);

  expect(screen.getByRole("button", { name: "Wake Sculpt Studio" })).toBeInTheDocument();
});

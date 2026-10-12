import { fireEvent, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";

import RuntimeUpdateCards from "./RuntimeUpdateCards";
import { runtimeUpdates } from "./runtimeUpdates";
import type { DevelopmentRuntime, Health, TerminalHostStatus } from "../api";

const health = { status: "ok", version: "1.0.0", degraded: [], worker_engine_build_id: "engine-new" } as Health;
const host: TerminalHostStatus = { protocol_version: 17, host_version: "1.0.0", host_build_id: "engine-old", draining: false, running_sessions: 8, retained_sessions: 8 };
const development = { enabled: true, version: "1.0.0", state: "idle", reload_available: true, deployed_source_revision: "e46308f51643", source_revision: "f15be478a2c1", source_dirty: false, deployed_source_published: true } as DevelopmentRuntime;

/**
 * ⚠️ THE OPERATOR'S 2026-10-12 SCREEN: both updates deferred on purpose, and as
 * full cards they left the worker list at nothing. Each is now one line that
 * still names its action and, for the engine, what it costs.
 */
test("updates the operator is deferring are one line each, not red, and still one press from running", () => {
  const onRun = vi.fn();
  const onDetails = vi.fn();
  const { container } = render(<RuntimeUpdateCards updates={runtimeUpdates(health, host, development)} busy={false} onRun={onRun} onDetails={onDetails} />);

  expect(container.querySelectorAll(".runtime-update-card.compact")).toHaveLength(2);
  expect(container.querySelector(".costly")).toBeNull();
  // The long explanations live in Settings, a press away.
  expect(screen.queryByText(/installed but not running/)).not.toBeInTheDocument();
  expect(screen.getByText("Restarts loaded workers")).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "Apply worker engine update" }));
  expect(onRun).toHaveBeenCalledWith(expect.objectContaining({ action: "apply_worker_engine" }));
  fireEvent.click(screen.getByRole("button", { name: "Build and release" }));
  expect(onRun).toHaveBeenLastCalledWith(expect.objectContaining({ action: "build" }));
  fireEvent.click(screen.getByRole("button", { name: "App and API update" }));
  expect(onDetails).toHaveBeenCalled();
});

test("a failed build and work in progress keep the full card with their explanation", () => {
  const failed = runtimeUpdates(undefined, undefined, { ...development, state: "failed", failure_detail: "cargo build exited 101" } as DevelopmentRuntime);
  const draining = runtimeUpdates(health, { ...host, draining: true }, undefined);
  const { container } = render(<RuntimeUpdateCards updates={[...draining, ...failed]} busy={false} onRun={vi.fn()} onDetails={vi.fn()} />);

  expect(container.querySelectorAll(".runtime-update-card.compact")).toHaveLength(0);
  expect(screen.getByText(/The worker engine is being replaced/)).toBeInTheDocument();
  expect(screen.getByText(/cargo build exited 101/)).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Build again" })).toBeInTheDocument();
});

test("nothing can be started while another runtime action is running", () => {
  render(<RuntimeUpdateCards updates={runtimeUpdates(health, host, development)} busy onRun={vi.fn()} onDetails={vi.fn()} />);
  expect(screen.getByRole("button", { name: "Apply worker engine update" })).toBeDisabled();
  expect(screen.getByRole("button", { name: "Build and release" })).toBeDisabled();
});

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import type { Worker } from "../api";
import WorkerRosterItem from "./WorkerRosterItem";

afterEach(cleanup);

function worker(): Worker {
  return {
    id: "019ff136-7a90-7631-bbc0-f95efd1df576",
    hive_id: "hive-1",
    name: "Platform",
    description: "",
    role: "worker",
    provider: "claude_code",
    workspace: "/home/bschleifer/projects/rcg/rcg-platform",
    autostart: false,
    position: 0,
    active_session_id: "01a02c3b-88a4-7ba0-8e71-7de5383c825a",
    created_at: 1,
    updated_at: 1,
    running: true,
    attention_state: "resting",
    last_output_at: 1,
  } as Worker;
}

/**
 * A short save disables the roster for a moment. Greying out every worker with
 * no reason attached to any of them reads as the roster having broken, so the
 * reason travels with the control that is inert.
 */
test("a disabled worker says why it is disabled", () => {
  render(
    <WorkerRosterItem
      worker={worker()}
      selected={false}
      detail="rcg-platform · Ready for work"
      busy
      busyReason="Saving the worker…"
      onOpen={vi.fn()}
      onStart={vi.fn()}
      onStop={vi.fn()}
    />,
  );

  const [open, menu] = screen.getAllByRole("button", { name: /Platform/ });
  expect(open).toBeDisabled();
  expect(menu).toBeDisabled();
  expect(open).toHaveAttribute("title", expect.stringContaining("Saving the worker…"));
  expect(open).toHaveAttribute("title", expect.stringContaining("Platform keeps running"));
});

test("an idle roster carries no explanation to give", () => {
  render(
    <WorkerRosterItem
      worker={worker()}
      selected={false}
      detail="rcg-platform · Ready for work"
      busy={false}
      onOpen={vi.fn()}
      onStart={vi.fn()}
      onStop={vi.fn()}
    />,
  );

  const [open] = screen.getAllByRole("button", { name: /Platform/ });
  expect(open).toBeEnabled();
  expect(open).not.toHaveAttribute("title");
});

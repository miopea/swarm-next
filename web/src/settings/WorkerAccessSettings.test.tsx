import { fireEvent, render, screen, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";

import type { Worker } from "../api";
import WorkerAccessSettings from "./WorkerAccessSettings";

const worker = (over: Partial<Worker> & Pick<Worker, "id" | "name">): Worker => ({
  hive_id: "hive", role: "worker", provider: "claude_code", workspace: `/${over.id}`, autostart: false,
  board_read: false, system_access: "none", position: 0, active_session_id: null, running: false,
  attention_state: "resting", created_at: 1, updated_at: 1, ...over,
});

test("says plainly when no worker may do more than its own task", () => {
  render(<WorkerAccessSettings workers={[worker({ id: "w1", name: "Hub" })]} onShowWorker={vi.fn()} />);

  expect(screen.getByText("No worker can read the whole board or run machine commands without asking you.")).toBeInTheDocument();
  expect(screen.queryByRole("list")).not.toBeInTheDocument();
});

/**
 * The few that hold anything, and only them. The roster already marks those
 * rows, but among every other row; this is where they can be read together.
 */
test("lists only the workers holding raised access, and links each to its own settings", () => {
  const onShowWorker = vi.fn();
  render(
    <WorkerAccessSettings
      workers={[
        worker({ id: "w1", name: "Hub" }),
        worker({ id: "w2", name: "Scout", board_read: true, system_access: "inspect" }),
        worker({ id: "w3", name: "Ops", system_access: "full" }),
      ]}
      onShowWorker={onShowWorker}
    />,
  );

  const rows = within(screen.getByRole("list")).getAllByRole("listitem");
  expect(rows.map((row) => row.querySelector("strong")?.textContent)).toEqual(["Scout", "Ops"]);
  expect(rows[0]).toHaveTextContent("reads the whole board");
  expect(rows[0]).toHaveTextContent("can look at the machine");
  expect(rows[1]).toHaveTextContent("runs any command");

  fireEvent.click(screen.getByRole("button", { name: "Edit Ops" }));
  expect(onShowWorker).toHaveBeenCalledWith("w3");
});

/**
 * Only a Claude launch writes a worker's commands (ADR 0132). A Codex worker
 * set to "full" runs none of them unasked, and saying it does would be false;
 * hiding it would hide what it gains the day it moves to Claude.
 */
test("marks commands set on a worker whose provider does not use them", () => {
  render(
    <WorkerAccessSettings
      workers={[
        worker({ id: "w1", name: "Codex worker", provider: "codex", system_access: "full" }),
        worker({ id: "w2", name: "Codex reader", provider: "codex", board_read: true }),
      ]}
      onShowWorker={vi.fn()}
    />,
  );

  const [commands, reader] = within(screen.getByRole("list")).getAllByRole("listitem");
  expect(commands).toHaveTextContent("Its commands are not in effect: only Claude workers use them.");
  expect(reader).not.toHaveTextContent("not in effect");
});

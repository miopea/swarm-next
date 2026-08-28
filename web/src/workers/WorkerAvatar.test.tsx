import { cleanup, render } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";

import type { Worker } from "../api";
import { markFor } from "../brand/beeMarks";
import WorkerAvatar from "./WorkerAvatar";

const worker: Worker = {
  id: "worker-daisy", hive_id: "hive-1", name: "Daisy", role: "worker", provider: "claude_code",
  workspace: "/workspace/daisy", autostart: false, position: 1, active_session_id: "session-1",
  running: true, attention_state: "buzzing", created_at: 1, updated_at: 1,
};

afterEach(cleanup);

/**
 * The marks exist so one repository's worker can be told from another's. The
 * mobile picker passed only the expression, so every worker there wore the
 * default bee — measured 2026-08-28 as 23 BeeMascot call sites with 3 marks.
 */
test("a worker wears its own mark, not the default", () => {
  const { container } = render(<WorkerAvatar worker={worker} />);
  const bee = container.querySelector(".bee-mascot")?.getAttribute("class") ?? "";
  expect(bee).toContain(`bee-mark-${markFor(worker.id)}`);
  expect(bee).not.toContain("bee-mark-plain");
});

/** An operator's chosen mark wins over the derived one. */
test("a chosen mark is honoured", () => {
  const { container } = render(<WorkerAvatar worker={{ ...worker, mark: "monocle" }} />);
  expect(container.querySelector(".bee-mascot")?.getAttribute("class")).toContain("bee-mark-monocle");
});

/**
 * ROLE IS DERIVED HERE RATHER THAN PASSED, because a caller that forgets it
 * draws the Queen as a worker — the defect efa4ee4 fixed for the control room
 * and the mobile picker still had.
 */
test("the Queen is drawn as the Queen without the caller saying so", () => {
  const { container } = render(<WorkerAvatar worker={{ ...worker, id: "queen", name: "Queen", role: "queen" }} />);
  const bee = container.querySelector(".bee-mascot")?.getAttribute("class") ?? "";
  expect(bee).toContain("bee-queen");
  expect(bee).not.toContain("bee-worker");
});

/** The presence pill is opt-in, so surfaces without one do not grow a stray dot. */
test("presence is rendered only when asked for", () => {
  const { container: without } = render(<WorkerAvatar worker={worker} />);
  expect(without.querySelector(".presence")).toBeNull();
  cleanup();
  const { container: with_ } = render(<WorkerAvatar worker={worker} presence />);
  expect(with_.querySelector(".presence")).not.toBeNull();
});

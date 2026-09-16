import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import WorkerCannotRunCard from "./WorkerCannotRunCard";

afterEach(cleanup);

const worker = (over: Partial<{ id: string; name: string; runtime_error?: string }> = {}) => ({
  id: "worker-1", name: "Queen", ...over,
});

/**
 * ⚠️ THE OPERATOR FOUND A DEAD QUEEN BY NOTICING A CHIP AND ASKING WHAT IT MEANT.
 *
 * When automatic recovery gives up, the worker records a runtime error and stays
 * down. That used to appear only as a Blocked chip on the roster and a WARN in
 * the journal — nothing that reaches someone not already looking. With Queen it
 * means routing, review and dispatch have all stopped.
 */
test("a worker that cannot run says so, and says nobody is retrying it", () => {
  render(
    <WorkerCannotRunCard
      workers={[worker({ runtime_error: "Worker exited again before recovery was stable." })]}
      onOpen={vi.fn()}
    />,
  );

  expect(screen.getByRole("region", { name: "A worker cannot run" })).toBeInTheDocument();
  expect(screen.getByText(/exited again before recovery was stable/)).toBeInTheDocument();
  // The difference between "it will come back" and "it will not" is the whole point.
  expect(screen.getByText(/stay down until you start them/)).toBeInTheDocument();
});

test("a healthy fleet renders nothing at all", () => {
  const { container } = render(
    <WorkerCannotRunCard workers={[worker(), worker({ id: "worker-2", name: "Scout" })]} onOpen={vi.fn()} />,
  );

  expect(container).toBeEmptyDOMElement();
});

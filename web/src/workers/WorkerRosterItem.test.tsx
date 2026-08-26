import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import type { Worker } from "../api";
import WorkerRosterItem from "./WorkerRosterItem";

const queen: Worker = {
  id: "queen", hive_id: "hive-1", name: "Queen", role: "queen", provider: "claude_code", workspace: "/workspace/queen",
  autostart: true, position: 0, active_session_id: "queen-session", running: true, attention_state: "buzzing", created_at: 1, updated_at: 1,
};

afterEach(cleanup);

test("right click opens the same accessible action menu and protects Queen", () => {
  const onOpen = vi.fn();
  render(<WorkerRosterItem worker={queen} selected detail="Always active" busy={false} onOpen={onOpen} onStart={vi.fn()} onStop={vi.fn()} />);

  fireEvent.contextMenu(screen.getByRole("button", { name: /Queen Buzzing.*Always active/ }));

  expect(screen.getByRole("menu", { name: "Queen actions" })).toBeInTheDocument();
  expect(screen.getByText("Queen is always active")).toBeInTheDocument();
  expect(screen.queryByRole("menuitem", { name: "Put worker to sleep" })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("menuitem", { name: "Open worker" }));
  expect(onOpen).toHaveBeenCalledOnce();
});

test("a running worker exposes stop through its visible menu", () => {
  const onStop = vi.fn();
  render(<WorkerRosterItem worker={{ ...queen, id: "worker", name: "Daisy", role: "worker" }} selected={false} detail="Running" busy={false} onOpen={vi.fn()} onStart={vi.fn()} onStop={onStop} />);

  fireEvent.click(screen.getByRole("button", { name: "Actions for Daisy" }));
  fireEvent.pointerDown(document.body);
  expect(screen.queryByRole("menu", { name: "Daisy actions" })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Actions for Daisy" }));
  fireEvent.click(screen.getByRole("menuitem", { name: "Put worker to sleep" }));

  expect(onStop).toHaveBeenCalledOnce();
  expect(screen.queryByRole("menu", { name: "Daisy actions" })).not.toBeInTheDocument();
});

test("shows operator engagement as a distinct scannable state", () => {
  render(<WorkerRosterItem worker={{ ...queen, attention_state: "with_operator", engagement_expires_at: Math.floor(Date.now() / 1000) + 300 }} selected detail="Direct steering" busy={false} onOpen={vi.fn()} onStart={vi.fn()} onStop={vi.fn()} />);

  expect(screen.getByText("With you")).toBeInTheDocument();
  expect(screen.getByTitle("With you")).toHaveClass("engaged");
});

test("shows terminal state separately from assigned work state", () => {
  render(<WorkerRosterItem worker={{ ...queen, attention_state: "resting" }} selected detail="Repair the release" workSummary="1 active · 2 ready" busy={false} onOpen={vi.fn()} onStart={vi.fn()} onStop={vi.fn()} />);

  expect(screen.getByText("Resting")).toBeInTheDocument();
  expect(screen.getByText("1 active · 2 ready")).toHaveAttribute("title", "Open work: 1 active · 2 ready");
});

test("shows a durable operator decision as awaiting you", () => {
  render(<WorkerRosterItem worker={{ ...queen, attention_state: "awaiting_operator" }} selected={false} detail="Decision requested" busy={false} onOpen={vi.fn()} onStart={vi.fn()} onStop={vi.fn()} />);

  expect(screen.getByText("Awaiting you")).toBeInTheDocument();
  expect(screen.getByTitle("Awaiting you")).toHaveClass("waiting");
});

test("offers an explicit retry after a worker launch failure", () => {
  const onStart = vi.fn();
  render(<WorkerRosterItem worker={{ ...queen, active_session_id: null, running: false, attention_state: "blocked", runtime_error: "Worker exited again before recovery was stable. Retry when ready." }} selected={false} detail="Worker exited again before recovery was stable. Retry when ready." busy={false} onOpen={vi.fn()} onStart={onStart} onStop={vi.fn()} />);

  expect(screen.getByText("Blocked")).toBeInTheDocument();
  expect(screen.getByText(/Retry when ready/)).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Actions for Queen" }));
  fireEvent.click(screen.getByRole("menuitem", { name: "Retry worker" }));
  expect(onStart).toHaveBeenCalledOnce();
});

test("returns to resting when the operator engagement lease expires", () => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date(100_000));
  const { container } = render(<WorkerRosterItem worker={{ ...queen, attention_state: "with_operator", engagement_expires_at: 101 }} selected detail="Direct steering" busy={false} onOpen={vi.fn()} onStart={vi.fn()} onStop={vi.fn()} />);
  const item = within(container);

  expect(item.getByText("With you")).toBeInTheDocument();
  act(() => vi.advanceTimersByTime(1_000));
  expect(item.getByText("Resting")).toBeInTheDocument();
  expect(item.getByTitle("Resting")).toHaveClass("online");
  vi.useRealTimers();
});

test("keeps the row on the three columns its grid defines", () => {
  // The row grid is `34px minmax(0, 1fr) 8px`. A fourth direct child wraps onto
  // an implicit second row and drags the presence dot with it, which is how the
  // unconfirmed-delivery mark first broke the roster layout.
  render(
    <WorkerRosterItem
      worker={{ ...queen, id: "worker", name: "Daisy", role: "worker", unconfirmed_delivery: true }}
      selected={false}
      detail="Running"
      busy={false}
      onOpen={vi.fn()}
      onStart={vi.fn()}
      onStop={vi.fn()}
    />,
  );

  const row = document.querySelector(".worker-button");
  expect(row?.children).toHaveLength(3);
});

test("marks a worker whose briefing Swarm could not confirm", () => {
  render(
    <WorkerRosterItem
      worker={{ ...queen, id: "worker", name: "Daisy", role: "worker", unconfirmed_delivery: true }}
      selected={false}
      detail="Running"
      busy={false}
      onOpen={vi.fn()}
      onStart={vi.fn()}
      onStop={vi.fn()}
    />,
  );

  expect(
    screen.getByRole("img", { name: "Swarm could not confirm this worker received its briefing" }),
  ).toBeInTheDocument();
});

test("stays quiet when the briefing was confirmed", () => {
  render(
    <WorkerRosterItem
      worker={{ ...queen, id: "worker", name: "Daisy", role: "worker" }}
      selected={false}
      detail="Running"
      busy={false}
      onOpen={vi.fn()}
      onStart={vi.fn()}
      onStop={vi.fn()}
    />,
  );

  expect(screen.queryByRole("img", { name: /could not confirm/ })).not.toBeInTheDocument();
});

/**
 * The shell is offered on the worker, but it is not a worker ACTION.
 *
 * Opening one must not wake, stop, or otherwise touch the worker — it borrows
 * the workspace path and nothing else. Asserting the other handlers stay
 * untouched is the real content here; that a click calls its own callback is
 * the trivial half.
 */
test("a shell can be opened from a worker without touching that worker", () => {
  const onOpenShell = vi.fn();
  const onStart = vi.fn();
  const onStop = vi.fn();
  render(
    <WorkerRosterItem
      worker={{ ...queen, id: "worker", name: "Daisy", role: "worker" }}
      selected={false}
      detail="Running"
      busy={false}
      onOpen={vi.fn()}
      onStart={onStart}
      onStop={onStop}
      onOpenShell={onOpenShell}
    />,
  );

  fireEvent.click(screen.getByRole("button", { name: "Actions for Daisy" }));
  fireEvent.click(screen.getByRole("menuitem", { name: "Open a shell here" }));

  expect(onOpenShell).toHaveBeenCalledTimes(1);
  expect(onStart).not.toHaveBeenCalled();
  expect(onStop).not.toHaveBeenCalled();
});

/** A caller with nowhere to put a terminal is not offered one. */
test("no shell entry appears when the caller cannot show one", () => {
  render(<WorkerRosterItem worker={{ ...queen, id: "worker", name: "Daisy", role: "worker" }} selected={false} detail="Running" busy={false} onOpen={vi.fn()} onStart={vi.fn()} onStop={vi.fn()} />);

  fireEvent.click(screen.getByRole("button", { name: "Actions for Daisy" }));

  expect(screen.queryByRole("menuitem", { name: "Open a shell here" })).toBeNull();
});

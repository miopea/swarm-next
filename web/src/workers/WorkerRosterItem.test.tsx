import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";

import type { Worker } from "../api";
import WorkerRosterItem from "./WorkerRosterItem";

const queen: Worker = {
  id: "queen", hive_id: "hive-1", name: "Queen", role: "queen", provider: "claude_code", workspace: "/workspace/queen",
  autostart: true, position: 0, active_session_id: "queen-session", running: true, attention_state: "buzzing", created_at: 1, updated_at: 1,
};

test("right click opens the same accessible action menu and protects Queen", () => {
  const onOpen = vi.fn();
  render(<WorkerRosterItem worker={queen} selected detail="Always active" busy={false} onOpen={onOpen} onStart={vi.fn()} onStop={vi.fn()} />);

  fireEvent.contextMenu(screen.getByRole("button", { name: /Queen Buzzing.*Always active/ }));

  expect(screen.getByRole("menu", { name: "Queen actions" })).toBeInTheDocument();
  expect(screen.getByText("Queen is always active")).toBeInTheDocument();
  expect(screen.queryByRole("menuitem", { name: "Stop worker" })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("menuitem", { name: "Open terminal" }));
  expect(onOpen).toHaveBeenCalledOnce();
});

test("a running worker exposes stop through its visible menu", () => {
  const onStop = vi.fn();
  render(<WorkerRosterItem worker={{ ...queen, id: "worker", name: "Daisy", role: "worker" }} selected={false} detail="Running" busy={false} onOpen={vi.fn()} onStart={vi.fn()} onStop={onStop} />);

  fireEvent.click(screen.getByRole("button", { name: "Actions for Daisy" }));
  fireEvent.pointerDown(document.body);
  expect(screen.queryByRole("menu", { name: "Daisy actions" })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Actions for Daisy" }));
  fireEvent.click(screen.getByRole("menuitem", { name: "Stop worker" }));

  expect(onStop).toHaveBeenCalledOnce();
  expect(screen.queryByRole("menu", { name: "Daisy actions" })).not.toBeInTheDocument();
});

test("shows operator engagement as a distinct scannable state", () => {
  render(<WorkerRosterItem worker={{ ...queen, attention_state: "with_operator", engagement_expires_at: Math.floor(Date.now() / 1000) + 300 }} selected detail="Direct steering" busy={false} onOpen={vi.fn()} onStart={vi.fn()} onStop={vi.fn()} />);

  expect(screen.getByText("With you")).toBeInTheDocument();
  expect(screen.getByTitle("With you")).toHaveClass("engaged");
});

test("shows a durable operator decision as awaiting you", () => {
  render(<WorkerRosterItem worker={{ ...queen, attention_state: "awaiting_operator" }} selected={false} detail="Decision requested" busy={false} onOpen={vi.fn()} onStart={vi.fn()} onStop={vi.fn()} />);

  expect(screen.getByText("Awaiting you")).toBeInTheDocument();
  expect(screen.getByTitle("Awaiting you")).toHaveClass("waiting");
});

test("returns to buzzing when the operator engagement lease expires", () => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date(100_000));
  const { container } = render(<WorkerRosterItem worker={{ ...queen, attention_state: "with_operator", engagement_expires_at: 101 }} selected detail="Direct steering" busy={false} onOpen={vi.fn()} onStart={vi.fn()} onStop={vi.fn()} />);
  const item = within(container);

  expect(item.getByText("With you")).toBeInTheDocument();
  act(() => vi.advanceTimersByTime(1_000));
  expect(item.getByText("Buzzing")).toBeInTheDocument();
  expect(item.getByTitle("Buzzing")).toHaveClass("online");
  vi.useRealTimers();
});

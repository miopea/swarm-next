import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { Task, TaskCi } from "../api";
import TaskCiStatus, { checkSummary, ciSummary, scopeSummary } from "./TaskCiStatus";

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

const observed = (outcome: object) => ({ commits: "1111aaaa2222", judged: ["1111aaaa2222bbbb"], observed_at: 1, ...outcome }) as TaskCi["observation"];

/** What a task's CI reads as on the board (ADR 0115). */
test("CI reads as what it means for closing the work", () => {
  const require = (observation: TaskCi["observation"]): TaskCi => ({ level: "require", recorded_commits: 1, observation, independent_check: null, must_not_touch: null });
  expect(ciSummary({ level: "off", recorded_commits: 3, observation: null, independent_check: null, must_not_touch: null })).toBeUndefined();
  expect(ciSummary({ level: "require", recorded_commits: 0, observation: null, independent_check: null, must_not_touch: null })).toBeUndefined();
  expect(ciSummary(require(null))).toEqual({ tone: "waiting", text: "Not read yet. It can't close until it is." });
  expect(ciSummary(require(observed({ outcome: "passed" })))).toEqual({ tone: "good", text: "Passed on 1111aaaa." });
  expect(ciSummary(require(observed({ outcome: "failed", failing: ["rust", "web"] }))))
    .toEqual({ tone: "bad", text: "Failed on 1111aaaa: rust, web. It can't close until this passes." });
  expect(ciSummary({ level: "warn", recorded_commits: 1, observation: observed({ outcome: "failed", failing: ["rust"] }), independent_check: null, must_not_touch: null }))
    .toEqual({ tone: "bad", text: "Failed on 1111aaaa: rust." });
  expect(ciSummary(require(observed({ outcome: "pending", waiting: ["the push to GitHub"] }))))
    .toEqual({ tone: "waiting", text: "Waiting for the push to GitHub on 1111aaaa. It can't close until CI passes." });
  expect(ciSummary(require(observed({ outcome: "unreadable", why: "gh is not logged in on this machine" }))))
    .toEqual({ tone: "unchecked", text: "Not checked: gh is not logged in on this machine." });
});

/** The worker's independent check reads as a fact beside the work, never as a gate (ADR 0171). */
test("the independent check reads as checked, stale or not checked", () => {
  expect(checkSummary(null)).toBeUndefined();
  expect(checkSummary({ state: "not_checked" }))
    .toEqual({ tone: "unchecked", word: "Not checked", text: "No independent check is recorded for these commits." });
  const checked = { state: "checked", head: "1111aaaa2222bbbb", ran: "cargo test", findings: "", recorded_at: 1 } as const;
  expect(checkSummary({ ...checked, verdict: "pass" }))
    .toEqual({ tone: "good", word: "Passed", at: "1111aaaa", text: "Independent check at 1111aaaa: pass. Ran: cargo test." });
  expect(checkSummary({ ...checked, verdict: "partial", ran: "read only.", findings: "src/lib.rs:4 keeps the old path" }))
    .toEqual({ tone: "waiting", word: "Partial", at: "1111aaaa", text: "Independent check at 1111aaaa: partial. Ran: read only. Findings: src/lib.rs:4 keeps the old path." });
  expect(checkSummary({ ...checked, verdict: "fail", findings: "A test was deleted." })?.tone).toBe("bad");
  const stale = { state: "stale", head: "1111aaaa2222bbbb", verdict: "pass", recorded_at: 1 } as const;
  expect(checkSummary({ ...stale, since: ["3333cccc4444dddd"] }))
    .toEqual({ tone: "unchecked", word: "Stale", text: "Independent check at 1111aaaa (pass) does not cover a later commit: 3333cccc." });
  expect(checkSummary({ ...stale, since: ["3333cccc4444dddd", "5555eeee6666ffff"] })?.text)
    .toBe("Independent check at 1111aaaa (pass) does not cover 2 later commits: 3333cccc, 5555eeee.");
});

/**
 * The check is not part of the CI gate, so a Hive with the gate off still
 * shows it on the card, and shows no CI row beside it.
 */
test("the card shows the independent check with the CI gate off", async () => {
  const task: Task = { id: "petal", hive_id: "hive", title: "Petals", state: "review", description: "", operator_instruction: "", priority: "normal", workspace: "/demo", assigned_worker_id: null, assigned_session_id: null, position: 0, created_at: 1, updated_at: 1 };
  const ci: TaskCi = {
    level: "off",
    recorded_commits: 1,
    observation: null,
    independent_check: { state: "checked", head: "1111aaaa2222bbbb", verdict: "fail", ran: "cargo test", findings: "A test was deleted.", recorded_at: 1 },
    must_not_touch: null,
  };
  vi.stubGlobal("fetch", vi.fn(() => Promise.resolve({ ok: true, status: 200, json: () => Promise.resolve(ci) } as Response)));
  render(<dl><TaskCiStatus task={task} operatorToken="token" /></dl>);

  expect(await screen.findByText("Check")).toBeInTheDocument();
  // The verdict and the commit it covers are read off the card, not only from a tooltip.
  expect(screen.getByText("at 1111aaaa")).toBeVisible();
  expect(screen.getByText("Failed")).toHaveAttribute("title", "Independent check at 1111aaaa: fail. Ran: cargo test. Findings: A test was deleted.");
  expect(screen.queryByText("CI")).not.toBeInTheDocument();
});

/** A crossing of the task's must-not-touch paths reads as something to look at, never a failure (ADR 0172). */
test("the scope reads as within or crossed, and as nothing without patterns or commits", () => {
  expect(scopeSummary(null)).toBeUndefined();
  const report = { patterns: ["crates/swarm-terminal/**"], commits_compared: 2, crossings: [], more_commits: 0 };
  expect(scopeSummary({ ...report, commits_compared: 0 })).toBeUndefined();
  expect(scopeSummary(report)).toEqual({
    tone: "good",
    word: "Within",
    text: "None of the 2 recorded commits touch the paths this task must not touch: crates/swarm-terminal/**.",
  });
  expect(scopeSummary({ ...report, commits_compared: 1 })?.text)
    .toBe("The recorded commit touches none of the paths this task must not touch: crates/swarm-terminal/**.");
  const crossings = [{ commit: "3333cccc4444dddd", paths: [{ path: "crates/swarm-terminal/src/ipc.rs", pattern: "crates/swarm-terminal/**" }], more_paths: 4 }];
  expect(scopeSummary({ ...report, crossings })).toEqual({
    tone: "waiting",
    word: "Crossed",
    text: "A commit changed paths this task must not touch (crates/swarm-terminal/**): 3333cccc changed crates/swarm-terminal/src/ipc.rs and 4 more. Queen decides whether that was needed.",
  });
  expect(scopeSummary({ ...report, crossings, more_commits: 2 })?.text)
    .toBe("3 commits changed paths this task must not touch (crates/swarm-terminal/**): 3333cccc changed crates/swarm-terminal/src/ipc.rs and 4 more; and 2 more commits. Queen decides whether that was needed.");
});

test("the card shows a crossing as a Scope row beside CI", async () => {
  const task: Task = { id: "petal", hive_id: "hive", title: "Petals", state: "review", description: "", operator_instruction: "", priority: "normal", workspace: "/demo", assigned_worker_id: null, assigned_session_id: null, position: 0, created_at: 1, updated_at: 1 };
  const ci: TaskCi = {
    level: "off",
    recorded_commits: 1,
    observation: null,
    independent_check: null,
    must_not_touch: {
      patterns: ["packaging/**"],
      commits_compared: 1,
      crossings: [{ commit: "3333cccc4444dddd", paths: [{ path: "packaging/linux/swarm-package", pattern: "packaging/**" }], more_paths: 0 }],
      more_commits: 0,
    },
  };
  vi.stubGlobal("fetch", vi.fn(() => Promise.resolve({ ok: true, status: 200, json: () => Promise.resolve(ci) } as Response)));
  render(<dl><TaskCiStatus task={task} operatorToken="token" /></dl>);

  expect(await screen.findByText("Scope")).toBeInTheDocument();
  expect(screen.getByText("Crossed")).toHaveAttribute(
    "title",
    "A commit changed paths this task must not touch (packaging/**): 3333cccc changed packaging/linux/swarm-package. Queen decides whether that was needed.",
  );
  expect(screen.queryByText("Check")).not.toBeInTheDocument();
});

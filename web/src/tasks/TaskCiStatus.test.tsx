import { expect, test } from "vitest";
import type { TaskCi } from "../api";
import { ciSummary } from "./TaskCiStatus";

const observed = (outcome: object) => ({ commits: "1111aaaa2222", judged: ["1111aaaa2222bbbb"], observed_at: 1, ...outcome }) as TaskCi["observation"];

/** What a task's CI reads as on the board (ADR 0115). */
test("CI reads as what it means for closing the work", () => {
  const require = (observation: TaskCi["observation"]): TaskCi => ({ level: "require", recorded_commits: 1, observation });
  expect(ciSummary({ level: "off", recorded_commits: 3, observation: null })).toBeUndefined();
  expect(ciSummary({ level: "require", recorded_commits: 0, observation: null })).toBeUndefined();
  expect(ciSummary(require(null))).toEqual({ tone: "waiting", text: "Not read yet. It can't close until it is." });
  expect(ciSummary(require(observed({ outcome: "passed" })))).toEqual({ tone: "good", text: "Passed on 1111aaaa." });
  expect(ciSummary(require(observed({ outcome: "failed", failing: ["rust", "web"] }))))
    .toEqual({ tone: "bad", text: "Failed on 1111aaaa: rust, web. It can't close until this passes." });
  expect(ciSummary({ level: "warn", recorded_commits: 1, observation: observed({ outcome: "failed", failing: ["rust"] }) }))
    .toEqual({ tone: "bad", text: "Failed on 1111aaaa: rust." });
  expect(ciSummary(require(observed({ outcome: "pending", waiting: ["the push to GitHub"] }))))
    .toEqual({ tone: "waiting", text: "Waiting for the push to GitHub on 1111aaaa. It can't close until CI passes." });
  expect(ciSummary(require(observed({ outcome: "unreadable", why: "gh is not logged in on this machine" }))))
    .toEqual({ tone: "unchecked", text: "Not checked: gh is not logged in on this machine." });
});

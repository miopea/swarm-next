import { expect, test } from "vitest";

import { jiraMappingDrift } from "./jiraMappingDrift";
import type { JiraProjectStatus, JiraStatusMapping } from "../api/jira";

const status = (id: string, name: string, recommended: JiraProjectStatus["recommended_task_state"]): JiraProjectStatus =>
  ({ id, name, category_key: "new", recommended_task_state: recommended });
const mapped = (id: string, name: string, state: JiraStatusMapping["task_state"]): JiraStatusMapping =>
  ({ jira_status_id: id, jira_status_name: name, task_state: state });

test("reports the WWD binding's three stale mappings and nothing else", () => {
  // The real case: mapped 2026-08-15, and the rule reading "Waiting On" as
  // blocked landed four days later. Three of ten have disagreed since.
  const stored = [
    mapped("1", "To Do", "ready"),
    mapped("2", "Is Next", "ready"),
    mapped("3", "Backlog", "ready"),
    mapped("4", "Waiting On", "ready"),
    mapped("5", "In Progress", "active"),
    mapped("6", "In Review", "active"),
    mapped("7", "Proofing", "active"),
    mapped("8", "Go Live", "active"),
    mapped("9", "Done", "completed"),
    mapped("10", "Removed", "completed"),
  ];
  const statuses = [
    status("1", "To Do", "ready"),
    status("2", "Is Next", "ready"),
    status("3", "Backlog", "draft"),
    status("4", "Waiting On", "blocked"),
    status("5", "In Progress", "active"),
    status("6", "In Review", "review"),
    status("7", "Proofing", "review"),
    status("8", "Go Live", "active"),
    status("9", "Done", "completed"),
    status("10", "Removed", "completed"),
  ];

  const drift = jiraMappingDrift(stored, statuses);

  expect(drift.map((entry) => entry.jira_status_name)).toEqual([
    "Backlog", "Waiting On", "In Review", "Proofing",
  ]);
  expect(drift.find((entry) => entry.jira_status_name === "Waiting On")).toMatchObject({
    stored: "ready",
    recommended: "blocked",
  });
});

test("a mapping that still matches is not drift", () => {
  const drift = jiraMappingDrift([mapped("1", "To Do", "ready")], [status("1", "To Do", "ready")]);
  expect(drift).toEqual([]);
});

test("a status this binding never mapped is not drift", () => {
  // Unmapped is a different problem with a different fix. Reporting it here
  // would bury the drift among it.
  const drift = jiraMappingDrift([], [status("9", "Newly Added", "ready")]);
  expect(drift).toEqual([]);
});

test("the live Jira name is shown, because that is what the operator recognises", () => {
  const drift = jiraMappingDrift(
    [mapped("4", "Waiting On Someone", "ready")],
    [status("4", "Waiting On", "blocked")],
  );
  expect(drift[0].jira_status_name).toBe("Waiting On");
});

import type { JiraProjectStatus, JiraStatusMapping } from "../api/jira";
import type { TaskState } from "../api";

/** One status whose stored mapping is not what Swarm would choose today. */
export type JiraMappingDrift = {
  jira_status_id: string;
  jira_status_name: string;
  /** What this Hive stored, and is still acting on. */
  stored: TaskState;
  /** What Swarm would recommend for this status now. */
  recommended: TaskState;
};

/**
 * Where a binding's stored mapping has fallen behind the recommendation.
 *
 * A binding is mapped once, at creation, and nothing re-applies the
 * recommendation when the recommendation improves. The WWD binding was mapped
 * on 2026-08-15; the rule that reads "Waiting On" as blocked landed four days
 * later. Three of its ten statuses have disagreed with the code ever since and
 * nothing said so — a Jira status meaning "blocked" was presenting as Ready
 * work, in the column people pick from.
 *
 * REPORTS ONLY. Nothing here rewrites a mapping, and that is the design rather
 * than a limitation: an operator may have overridden a recommendation on
 * purpose, drift is not the same as error, and a system that quietly corrects a
 * deliberate choice is worse than one that never notices. Showing both values
 * and leaving the decision alone cannot get that wrong.
 *
 * A status Jira has but this binding never mapped is not drift — it is an
 * unmapped status, which is a different problem with a different fix, and
 * reporting it here would bury the drift among it.
 */
export function jiraMappingDrift(
  stored: JiraStatusMapping[],
  statuses: JiraProjectStatus[],
): JiraMappingDrift[] {
  const byId = new Map(stored.map((mapping) => [mapping.jira_status_id, mapping]));
  return statuses
    .map((status) => {
      const mapping = byId.get(status.id);
      if (!mapping || mapping.task_state === status.recommended_task_state) return undefined;
      return {
        jira_status_id: status.id,
        // The stored name is what this Hive is acting on; Jira's is what the
        // project calls it now. They can differ, and the operator needs to
        // recognise the row, so the live name wins.
        jira_status_name: status.name,
        stored: mapping.task_state,
        recommended: status.recommended_task_state,
      } satisfies JiraMappingDrift;
    })
    .filter((entry): entry is JiraMappingDrift => entry !== undefined);
}

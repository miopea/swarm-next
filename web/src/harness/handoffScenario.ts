/** Fictional wire responses only; never forwards a request to a Hive. */
export function handoffScenario() {
  const state = { unavailable: "", offered: false, posts: 0, hasTarget: true };
  const fetch = async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    if (init?.method === "POST") {
      if (!url.endsWith("/claims/claim-1/handoffs")) throw new Error("Unexpected fictional command");
      state.posts++;
      state.offered = true;
      throw new Error("Fictional lost response after Keeper saved the offer");
    }
    if (state.unavailable && url.endsWith(state.unavailable)) throw new Error("Fictional read outage");
    const value = url.endsWith("/members") ? []
      : url.endsWith("/shared-work") ? [{ id: "claim-1", apiary_id: "garden", project_id: "project", issue_id: "issue", issue_key: "DEMO-1", project_name: "Fictional garden", home_node_id: "node-2", home_hive_id: "hive-2", home_operator_id: "operator-2", home_operator_display_name: "Cora", state: "confirmed" }]
      : url.endsWith("/handoffs") ? state.offered ? [{ id: "handoff-1", claim_id: "claim-1", source_hive_id: "hive-2", target_hive_id: "hive-3", issue_key: "DEMO-1", state: "offered" }] : []
      : url.endsWith("/handoff-targets") ? state.hasTarget ? [{ node_id: "node-3", hive_id: "hive-3", hive_name: "Fern Hive", operator_id: "operator-3", operator_display_name: "Faye" }] : []
      : url.endsWith("/sync-health") ? { condition: "current", last_success_at: 100, consecutive_failures: 0 }
      : url.endsWith("/task-sync-status") ? { cursor: 0, task_count: 0 }
      : url.endsWith("/task-outbox-status") ? { queued_count: 0, conflict_count: 0, rejected_count: 0 }
      : url.endsWith("/catalog-readiness") ? { acknowledgement: null, jira_connection: "not_connected", projects: [], blockers: [] }
      : url.endsWith("/my-stewardship") ? null
      : url.endsWith("/steward/assists") ? { incoming: [], outbox: [] }
      : [];
    return new Response(JSON.stringify(value), { headers: { "Content-Type": "application/json" } });
  };
  return { state, fetch };
}

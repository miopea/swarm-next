import { useEffect, useState } from "react";
import ApiarySettings from "../settings/ApiarySettings";
import type { HiveIdentity } from "../api";

const identity = {
  operator: { id: "operator-2", display_name: "Cora" },
  hive: { id: "clover", name: "Clover Hive", operator_id: "operator-2", apiary_id: "garden" },
  apiary_context: { mode: "federated", local_role: "member", apiary: { id: "garden", name: "Meadow Test Garden", keeper_operator_id: "fictional", shared_work_backend: "jira" } },
} satisfies HiveIdentity;

/**
 * No-proxy fixture only: a member's Apiary settings while synchronization is
 * stopped at one step. All reads are fictional and no action reaches a Hive.
 */
export default function MemberSyncSettingsFixture() {
  const [mounted, setMounted] = useState(false);
  useEffect(() => {
    const previous = window.fetch;
    window.fetch = async (input) => {
      const url = String(input);
      const now = Math.floor(Date.now() / 1000);
      const payload = url.endsWith("/sync-health") ? { condition: "authentication_required", last_attempt_at: now - 20, last_success_at: now - 86_400, consecutive_failures: 6, next_attempt_at: now + 280, failed_step: "membership credential" }
        : url.endsWith("/catalog-readiness") ? { acknowledgement: null, jira_connection: "ready", projects: [], blockers: ["catalog_missing"] }
        : url.endsWith("/members") ? [
            { hive_id: "fictional", hive_name: "Meadow Hive", operator_id: "fictional", operator_display_name: "Bea", role: "keeper", is_local: false },
            { hive_id: "clover", hive_name: "Clover Hive", operator_id: "operator-2", operator_display_name: "Cora", role: "member", is_local: true },
          ]
        : url.endsWith("/departure-readiness") ? {
            state: "active", keeper_reachable: true,
            readiness: { apiary_id: "garden", member_node_id: "node-2", member_hive_id: "clover", active_jira_claim_count: 0, open_swarm_task_count: 0, active_stewardship_count: 0, pending_task_command_count: 0, pending_jira_claim_count: 0 },
          }
        : [];
      return new Response(JSON.stringify(payload), { headers: { "Content-Type": "application/json" } });
    };
    setMounted(true);
    return () => { window.fetch = previous; };
  }, []);
  return mounted ? <ApiarySettings busy={false} hiveIdentity={identity} operatorToken="fictional" onHiveIdentityChange={() => {}} /> : null;
}

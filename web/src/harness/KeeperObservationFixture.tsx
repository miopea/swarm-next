import { useEffect, useRef, useState } from "react";
import KeeperControlRoom from "../apiary/KeeperControlRoom";
import MemberControlRoom from "../apiary/MemberControlRoom";
import type { HiveIdentity } from "../api";

const identity = { operator: { id: "fictional", display_name: "Bea" }, hive: { id: "fictional", name: "Meadow Hive", operator_id: "fictional", apiary_id: "garden" }, apiary_context: { mode: "federated", local_role: "keeper", apiary: { id: "garden", name: "Meadow Test Garden", keeper_operator_id: "fictional", shared_work_backend: "jira" } } } satisfies HiveIdentity;

/** No-proxy fixture only: all reads are fictional and no action reaches a Hive. */
export default function KeeperObservationFixture({ member = false }: { member?: boolean }) {
  const mode = useRef<"failed" | "ready">("failed");
  const [mounted, setMounted] = useState(false);
  useEffect(() => {
    const previous = window.fetch;
    window.fetch = async (input) => {
      if (mode.current === "failed") throw new Error("Fictional observation failure");
      const url = String(input);
      const payload = url.endsWith("/catalog-readiness") ? { acknowledgement: null, jira_connection: "not_connected", projects: [], blockers: [] }
        : url.endsWith("/sync-health") ? { condition: "current", last_attempt_at: 100, last_success_at: 100, consecutive_failures: 0 }
        : url.endsWith("/task-sync-status") ? { cursor: 0, task_count: 0 }
        : url.endsWith("/task-outbox-status") ? { queued_count: 0, conflict_count: 0, rejected_count: 0 }
        : url.endsWith("/my-stewardship") ? null
        : url.endsWith("/steward/assists") ? { incoming: [], outbox: [] }
        // ⚠️ THIS USED TO RETURN THE LOCAL HIVE ALONE, so the roster's action
        // row — Watch, Take over, and the version line beside them — never
        // rendered here at all. That is why the surface built to let somebody
        // LOOK at this page could not have caught the clipped button shipped in
        // 1.13.0. A roster fixture with nothing to act on is a fixture of a
        // different page. Remote Hives now appear, in the standings an operator
        // has to be able to tell apart at a glance.
        : url.endsWith("/members")
        ? [
            { hive_id: "fictional", hive_name: "Meadow Hive", operator_id: "fictional", operator_display_name: "Bea", role: "keeper", is_local: true },
            { hive_id: "clover", hive_name: "Clover Hive", operator_id: "operator-2", operator_display_name: "Cora", operator_email: "cora@example.invalid", role: "member", is_local: false },
            { hive_id: "thistle", hive_name: "Thistle Hive", operator_id: "operator-3", operator_display_name: "Wren", operator_email: "wren@example.invalid", role: "member", is_local: false },
            { hive_id: "heather", hive_name: "Heather Hive", operator_id: "operator-4", operator_display_name: "Fen", role: "member", is_local: false },
          ]
        : url.endsWith("/fleet-versions")
        ? {
            expected_release: "1.13.1", expected_release_first_seen_at: 100, expected_schema_version: 192,
            hives: [
              { hive_id: "fictional", node_id: "node-1", swarm_version: "1.13.1", database_schema_version: 192, observed_at: 100, standing: "current", raises: false },
              { hive_id: "clover", node_id: "node-2", swarm_version: "1.13.1", database_schema_version: 192, observed_at: 100, standing: "current", raises: false },
              { hive_id: "thistle", node_id: "node-3", swarm_version: "1.11.0", database_schema_version: 188, observed_at: 100, standing: "schema_behind", raises: true },
              // Heather reports no version at all, which is the case that has to
              // read as "unknown" rather than as an empty gap in the row.
            ],
          }
        : [];
      return new Response(JSON.stringify(payload), { headers: { "Content-Type": "application/json" } });
    };
    setMounted(true);
    return () => { window.fetch = previous; };
  }, []);
  return <>
    <p>Fictional {member ? "Member" : "Keeper"} observations. These controls cannot reach a Hive.</p>
    <button type="button" onClick={() => { mode.current = "ready"; document.dispatchEvent(new Event("visibilitychange")); }}>Restore fictional reads</button>
    <button type="button" onClick={() => { mode.current = "failed"; document.dispatchEvent(new Event("visibilitychange")); }}>Fail fictional reads</button>
    {mounted ? member ? <MemberControlRoom identity={{ ...identity, apiary_context: { ...identity.apiary_context, local_role: "member" } }} operatorToken="fictional" onManage={() => {}} onOpenTasks={() => {}} /> : <KeeperControlRoom identity={identity} operatorToken="fictional" onManage={() => {}} onInvite={() => {}} onOpenTasks={() => {}} /> : null}
  </>;
}

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
    window.fetch = async (input, init) => {
      if (mode.current === "failed") throw new Error("Fictional observation failure");
      const url = String(input);
      const now = Math.floor(Date.now() / 1000);
      const line = (sequence: number, secondsAgo: number, level: string, target: string, message: string) => ({ sequence, at_ms: (now - secondsAgo) * 1000, level, target, message });
      // Opening a watch answers with the watch, as the real route does, so the
      // window knows which Hive it is waiting on.
      if (url.endsWith("/apiary/watches") && init?.method === "POST") {
        const target = (JSON.parse(String(init.body)) as { target_hive_id: string }).target_hive_id;
        return new Response(JSON.stringify({ id: "fictional-watch", target_hive_id: target, state: "requested", requested_at: now, acknowledged_at: null, expires_at: now + 300, ended_at: null }), { headers: { "Content-Type": "application/json" } });
      }
      const payload = url.endsWith("/diagnostics/log") ? { entries: [line(1, 90, "info", "swarm_api", "Swarm API listening address=127.0.0.1:8766")], dropped: 0, received_at: null }
        : url.endsWith("/diagnostics") ? {
            entries: [
              line(40, 300, "info", "swarm_api", "Swarm API listening address=127.0.0.1:8766"),
              line(41, 240, "warn", "swarm_api", "Apiary synchronisation stopped at this step; it retries on a backoff step=\"project catalog\" condition=Incompatible"),
              line(42, 180, "debug", "swarm_api::takeover_producer", "takeover relay connected to Keeper lease=01a0eee4-2874-7491-85c3-5bc21d940346 revision=2"),
              line(43, 120, "warn", "swarm_api::takeover_producer", "the takeover holder's keystrokes were refused lease=01a0eee4-2874-7491-85c3-5bc21d940346 reason=terminal_operation_failed: this terminal requires generation-bound control"),
              line(44, 60, "error", "swarm_api", "the terminal host did not answer error=connection refused"),
            ],
            dropped: 2, received_at: now - 20,
          }
        : url.endsWith("/catalog-readiness") ? { acknowledgement: null, jira_connection: "not_connected", projects: [], blockers: [] }
        // Stopped at one step and waiting out its backoff: the state a field
        // member sat in for days while saying only "needs attention".
        : url.endsWith("/sync-health") ? { condition: "incompatible", last_attempt_at: now - 20, last_success_at: now - 3 * 86_400, consecutive_failures: 4, next_attempt_at: now + 100, failed_step: "project catalog" }
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
            { hive_id: "clover", hive_name: "Clover Hive", operator_id: "operator-2", operator_display_name: "Cora", operator_email: "cora@example.invalid", role: "member", is_local: false, last_contact_at: now - 25 },
            // Silent for days while still listing the release it last reported —
            // how a Hive already updated went on reading as behind.
            { hive_id: "thistle", hive_name: "Thistle Hive", operator_id: "operator-3", operator_display_name: "Wren", operator_email: "wren@example.invalid", role: "member", is_local: false, last_contact_at: now - 3 * 86_400 },
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

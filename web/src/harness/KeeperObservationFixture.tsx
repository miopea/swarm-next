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
        : url.endsWith("/members")
        ? [{ hive_id: "fictional", hive_name: "Meadow Hive", operator_id: "fictional", operator_display_name: "Bea", role: "keeper", is_local: true }]
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

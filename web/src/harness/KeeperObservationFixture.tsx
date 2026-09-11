import { useEffect, useRef, useState } from "react";
import KeeperControlRoom from "../apiary/KeeperControlRoom";
import type { HiveIdentity } from "../api";

const identity: HiveIdentity = { operator: { id: "fictional", display_name: "Bea" }, hive: { id: "fictional", name: "Meadow Hive", operator_id: "fictional", apiary_id: "garden" }, apiary_context: { mode: "federated", local_role: "keeper", apiary: { id: "garden", name: "Meadow Test Garden", keeper_operator_id: "fictional", shared_work_backend: "jira" } } };

/** No-proxy fixture only: all reads are fictional and no action reaches a Hive. */
export default function KeeperObservationFixture() {
  const mode = useRef<"failed" | "ready">("failed");
  const [mounted, setMounted] = useState(false);
  useEffect(() => {
    const previous = window.fetch;
    window.fetch = async (input) => {
      if (mode.current === "failed") throw new Error("Fictional observation failure");
      const payload = String(input).endsWith("/members")
        ? [{ hive_id: "fictional", hive_name: "Meadow Hive", operator_id: "fictional", operator_display_name: "Bea", role: "keeper", is_local: true }]
        : [];
      return new Response(JSON.stringify(payload), { headers: { "Content-Type": "application/json" } });
    };
    setMounted(true);
    return () => { window.fetch = previous; };
  }, []);
  return <>
    <p>Fictional Keeper observations. These controls cannot reach a Hive.</p>
    <button type="button" onClick={() => { mode.current = "ready"; document.dispatchEvent(new Event("visibilitychange")); }}>Restore fictional reads</button>
    <button type="button" onClick={() => { mode.current = "failed"; document.dispatchEvent(new Event("visibilitychange")); }}>Fail fictional reads</button>
    {mounted ? <KeeperControlRoom identity={identity} operatorToken="fictional" onManage={() => {}} onInvite={() => {}} onOpenTasks={() => {}} /> : null}
  </>;
}

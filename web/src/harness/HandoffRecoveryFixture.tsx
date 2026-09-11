import { useEffect, useState } from "react";
import MemberControlRoom from "../apiary/MemberControlRoom";
import { handoffScenario } from "./handoffScenario";
import type { HiveIdentity } from "../api";

const identity = { operator: { id: "operator-2", display_name: "Cora" }, hive: { id: "hive-2", name: "Clover Hive", operator_id: "operator-2", apiary_id: "garden" }, apiary_context: { mode: "federated", local_role: "member", apiary: { id: "garden", name: "Fictional Handoff Garden", keeper_operator_id: "fictional", shared_work_backend: "jira" } } } satisfies HiveIdentity;

export default function HandoffRecoveryFixture() {
  const [scenario] = useState(handoffScenario);
  const [mounted, setMounted] = useState(false);
  useEffect(() => {
    const previous = window.fetch;
    window.fetch = scenario.fetch;
    setMounted(true);
    return () => { window.fetch = previous; };
  }, [scenario]);
  return <>
    <p>Fictional handoff recovery. No request reaches a Hive. Sending saves an offer but loses its reply.</p>
    <button onClick={() => { scenario.state.unavailable = "/handoffs"; }}>Fail handoff reads</button>
    <button onClick={() => { scenario.state.unavailable = "/handoff-targets"; }}>Fail recipient reads</button>
    <button onClick={() => { scenario.state.unavailable = ""; }}>Restore handoff reads</button>
    {mounted ? <MemberControlRoom identity={identity} operatorToken="fictional" onManage={() => {}} onOpenTasks={() => {}} /> : null}
  </>;
}

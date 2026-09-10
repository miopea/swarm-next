import { useEffect, useState } from "react";
import PersonalHiveJoin from "../settings/PersonalHiveJoin";

/** Fictional consent and membership only, served by the no-proxy harness. */
export default function JoinApiaryFixture() {
  const [ready, setReady] = useState(false);
  const [error, setError] = useState("");
  const [message, setMessage] = useState("");
  useEffect(() => {
    const previous = window.fetch;
    const invitation = {
      invitation_id: "fictional-invitation", apiary_name: "Clover Garden",
      keeper_hive_name: "Bea's Hive", keeper_operator_display_name: "Bea",
      required_policy_revision: 3, promoted_projects: [], state: "keeper_pinned",
      readiness: { jira_connection: "ready", projects: [], blockers: ["policy_not_accepted"] },
    };
    window.fetch = async (input, init) => {
      const path = String(input);
      if (path.endsWith("/policy-acceptance") && init?.method === "POST") {
        invitation.state = "policy_accepted";
        invitation.readiness.blockers = [];
        return new Response(JSON.stringify(invitation));
      }
      if (path.endsWith("/submission") && init?.method === "POST") return new Response(JSON.stringify({ kind: "federated" }));
      if (path.endsWith("/join-invitations")) return new Response(JSON.stringify([invitation]));
      return previous(input, init);
    };
    setReady(true);
    return () => { window.fetch = previous; };
  }, []);
  return <main className="settings-card">
    <h1>Join Clover Garden — fictional preview</h1>
    <p>Keeper approval and Jira checks are already complete. No real membership is changed.</p>
    {message && <p role="status">{message}</p>}
    {error && <p role="alert">{error}</p>}
    {ready && <PersonalHiveJoin busy={false} operatorToken="fixture" onError={setError} onMessage={setMessage}
      onJoined={async () => { throw new Error("Fictional post-join refresh failure"); }} />}
  </main>;
}

import type { FederationCatalogReadiness, FederationSyncHealth } from "../api";

export default function MemberSetup({ catalog, sync, onManage, onRefresh }: {
  catalog?: FederationCatalogReadiness;
  sync?: FederationSyncHealth;
  onManage: () => void;
  onRefresh: () => void;
}) {
  const readyProjects = catalog?.projects.filter((project) => project.binding_id && project.access_verified && project.workflow_mapped) ?? [];
  const remainingProjects = catalog?.projects.filter((project) => !project.binding_id || !project.access_verified || !project.workflow_mapped) ?? [];
  const policyChanged = catalog?.blockers.includes("policy_revision_changed");
  return <article className="keeper-panel member-setup" aria-labelledby="member-setup-heading">
    <header><div><p className="eyebrow">Make yourself at home</p><h4 id="member-setup-heading">Your Apiary setup</h4></div></header>
    <p>Your Hive is a member. Local tasks and workers remain on this Hive. Jira is an optional source of shared work.</p>
    {!sync || !catalog ? <p role="status">Checking shared setup. Missing status is not a failed membership.</p> : null}
    {sync?.condition === "current" ? <p><strong>Connected to Keeper</strong> · Latest synchronization completed.</p>
      : sync?.condition === "offline" ? <div><strong>Waiting on Keeper connection</strong><p>Your Hive will reconnect automatically. Local work can continue.</p><button className="secondary-button" onClick={onRefresh}>Check connection</button></div>
      : sync?.condition === "authentication_required" || sync?.condition === "incompatible" ? <div><strong>Shared connection needs attention</strong><p>Review the membership connection before shared changes can resume.</p><button className="secondary-button" onClick={onManage}>Review connection</button></div> : null}
    {policyChanged ? <div><strong>Keeper updated the shared policy</strong><p>Review what changed before accepting new authority.</p><button className="secondary-button" onClick={onManage}>Review policy</button></div> : null}
    {catalog?.jira_connection === "not_connected" ? <div><strong>Optional: connect Jira</strong><p>Add Jira work if your team uses it. You do not need Jira for Apiary membership.</p><a className="secondary-button" href="#settings-integrations">Connect Jira</a></div> : null}
    {catalog?.jira_connection === "network_unavailable" ? <div><strong>Jira is temporarily unreachable</strong><p>Your Apiary membership is unchanged. Check again when the connection returns; you do not need to join again.</p><button className="secondary-button" onClick={onRefresh}>Check Jira connection</button></div> : null}
    {catalog?.jira_connection === "credentials_invalid" ? <div><strong>Jira sign-in needs attention</strong><p>Restore Jira access to resume Jira work. Your Hive is still an Apiary member.</p><a className="secondary-button" href="#settings-integrations">Review Jira sign-in</a></div> : null}
    {catalog?.jira_connection === "permission_denied" ? <div><strong>Jira access needs review</strong><p>Check access to the projects you use. You do not need access to every Apiary project, and your membership is unchanged.</p><a className="secondary-button" href="#settings-integrations">Review Jira access</a></div> : null}
    {catalog?.jira_connection === "ready" ? <div><strong>{readyProjects.length} Jira {readyProjects.length === 1 ? "project ready" : "projects ready"}</strong>
        {remainingProjects.length > 0 ? <><p>{remainingProjects.length} shared {remainingProjects.length === 1 ? "project has" : "projects have"} additional setup or access requirements. Only configure the projects you use.</p><a className="secondary-button" href="#settings-integrations">Review Jira projects</a></> : null}</div> : null}
  </article>;
}

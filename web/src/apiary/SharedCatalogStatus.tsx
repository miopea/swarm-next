import type { FederationCatalogReadiness } from "../api";
import { catalogBlockerLabel, catalogReadinessLabel } from "./presentation";

/** Describes separate setup concerns; never authorizes a claim or hides a refusal. */
export default function SharedCatalogStatus({ catalog }: { catalog?: FederationCatalogReadiness }) {
  if (!catalog) return <p className="keeper-empty">Waiting for shared catalog status.</p>;
  const jira = catalog.blockers.filter((blocker) => blocker === "integration_not_ready" || blocker === "project_access_not_ready");
  const shared = catalog.blockers.filter((blocker) => blocker !== "integration_not_ready" && blocker !== "project_access_not_ready");
  const verified = catalogReadinessLabel(catalog) === "Verified";
  return <>
    {shared.length ? <ul className="member-blocker-list" aria-label="Shared work blockers">
      {shared.map((blocker) => <li key={blocker}>{catalogBlockerLabel(blocker)}</li>)}
    </ul> : <p className={verified ? "member-ready-copy" : "keeper-empty"}>{verified ? "Shared catalog prerequisites are ready." : "Waiting for shared catalog status."}</p>}
    {jira.length ? <div className="apiary-optional-setup">
      <strong>Optional Jira setup</strong>
      <p>These items affect Jira work, not Apiary membership. Configure only the projects you use.</p>
      <ul aria-label="Jira setup requirements">{jira.map((blocker) => <li key={blocker}>{blocker === "integration_not_ready" && catalog.jira_connection === "not_connected" ? "Connect Jira if you want to use Jira work" : catalogBlockerLabel(blocker)}</li>)}</ul>
      <a className="secondary-button" href="#settings-integrations">Review Jira setup</a>
    </div> : null}
  </>;
}

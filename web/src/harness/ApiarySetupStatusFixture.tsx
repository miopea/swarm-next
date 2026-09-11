import { useState } from "react";
import type { FederationCatalogReadiness } from "../api";
import SharedCatalogStatus from "../apiary/SharedCatalogStatus";

/** Fictional presentation only; no connection, consent or membership writes. */
export default function ApiarySetupStatusFixture() {
  const [state, setState] = useState("optional");
  const catalog: FederationCatalogReadiness = {
    acknowledgement: { apiary_id: "fictional", policy_revision: 1, promoted_project_catalog_digest: "fictional",
      project_count: 0, snapshot_issued_at: 1, snapshot_expires_at: 9999999999, acknowledged_at: 1 },
    jira_connection: state === "ready" ? "ready" : "not_connected", projects: [],
    blockers: state === "ready" ? [] : state === "mixed" ? ["catalog_stale", "policy_revision_changed", "integration_not_ready"] : ["integration_not_ready"],
  };
  return <section className="keeper-panel">
    <label>Fictional setup state <select value={state} onChange={(event) => setState(event.target.value)}>
      <option value="optional">No Jira connected</option><option value="mixed">Policy and Jira requirements</option>
      <option value="ready">Setup recovered</option><option value="missing">Status unavailable</option>
    </select></label>
    <h3>Shared setup</h3>
    <SharedCatalogStatus catalog={state === "missing" ? undefined : catalog} />
  </section>;
}

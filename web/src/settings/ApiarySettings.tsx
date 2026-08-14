import { useEffect, useMemo, useState } from "react";

import {
  collapseApiary,
  createApiary,
  fetchApiaryCollapseReadiness,
  fetchApiaryHiveCandidates,
  fetchApiaryJiraProjects,
  fetchFederationJoinInvitations,
  fetchHive,
  fetchHiveConnectionCard,
  fetchJiraBindings,
  importFederationJoinInvitation,
  inviteApiaryHiveCandidate,
  pinApiaryHiveCandidate,
  promoteApiaryJiraProject,
  type ApiaryCollapseReadiness,
  type ApiaryHiveCandidate,
  type ApiaryInvitationBundle,
  type ApiaryJiraProject,
  type FederationJoinInvitation,
  type HiveConnectionCard,
  type HiveIdentity,
  type JiraProjectBinding,
} from "../api";
import { downloadJson } from "../shared/download";

type Props = {
  busy: boolean;
  hiveIdentity: HiveIdentity | undefined;
  operatorToken: string;
  onHiveIdentityChange: (identity: HiveIdentity) => void;
};

export default function ApiarySettings({ busy, hiveIdentity, operatorToken, onHiveIdentityChange }: Props) {
  const context = hiveIdentity?.apiary_context;
  const personal = !context || context.mode === "personal";
  const [name, setName] = useState("");
  const [confirmCreate, setConfirmCreate] = useState(false);
  const [confirmCollapse, setConfirmCollapse] = useState(false);
  const [readiness, setReadiness] = useState<ApiaryCollapseReadiness>();
  const [promotedProjects, setPromotedProjects] = useState<ApiaryJiraProject[]>([]);
  const [hiveCandidates, setHiveCandidates] = useState<ApiaryHiveCandidate[]>([]);
  const [joinInvitations, setJoinInvitations] = useState<FederationJoinInvitation[]>([]);
  const [invitationPreview, setInvitationPreview] = useState<ApiaryInvitationBundle>();
  const [jiraBindings, setJiraBindings] = useState<JiraProjectBinding[]>([]);
  const [projectLoadError, setProjectLoadError] = useState(false);
  const [candidateLoadError, setCandidateLoadError] = useState(false);
  const [working, setWorking] = useState(false);
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");
  const keeper = context?.mode === "federated" && context.local_role === "keeper";

  useEffect(() => {
    if (!keeper) {
      setReadiness(undefined);
      return;
    }
    let cancelled = false;
    void fetchApiaryCollapseReadiness(operatorToken)
      .then((value) => { if (!cancelled) setReadiness(value); })
      .catch(() => { if (!cancelled) setReadiness(undefined); });
    return () => { cancelled = true; };
  }, [keeper, operatorToken, hiveIdentity?.hive.apiary_id]);

  useEffect(() => {
    if (!personal) {
      setJoinInvitations([]);
      setInvitationPreview(undefined);
      return;
    }
    let cancelled = false;
    void fetchFederationJoinInvitations(operatorToken)
      .then((value) => { if (!cancelled) setJoinInvitations(value); })
      .catch(() => { if (!cancelled) setJoinInvitations([]); });
    return () => { cancelled = true; };
  }, [personal, operatorToken, hiveIdentity?.hive.apiary_id]);

  useEffect(() => {
    if (!keeper) {
      setPromotedProjects([]);
      setHiveCandidates([]);
      setJiraBindings([]);
      setProjectLoadError(false);
      setCandidateLoadError(false);
      return;
    }
    let cancelled = false;
    void Promise.allSettled([
      fetchApiaryJiraProjects(operatorToken),
      fetchJiraBindings(operatorToken),
      fetchApiaryHiveCandidates(operatorToken),
    ])
      .then(([projects, bindings, candidates]) => {
        if (cancelled) return;
        setPromotedProjects(projects.status === "fulfilled" ? projects.value : []);
        setJiraBindings(bindings.status === "fulfilled" ? bindings.value : []);
        setHiveCandidates(candidates.status === "fulfilled" ? candidates.value : []);
        setProjectLoadError(projects.status === "rejected" || bindings.status === "rejected");
        setCandidateLoadError(candidates.status === "rejected");
      });
    return () => { cancelled = true; };
  }, [keeper, operatorToken, hiveIdentity?.hive.apiary_id]);

  const blockers = useMemo(() => collapseBlockers(readiness), [readiness]);

  async function refreshIdentity() {
    const identity = await fetchHive(operatorToken);
    onHiveIdentityChange(identity);
  }

  async function foundApiary() {
    setWorking(true);
    setError("");
    setMessage("");
    try {
      await createApiary(operatorToken, name.trim(), "jira");
      await refreshIdentity();
      setConfirmCreate(false);
      setMessage(`${name.trim()} is now a Jira-backed Apiary.`);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "The Apiary could not be created.");
    } finally {
      setWorking(false);
    }
  }

  async function downloadConnectionCard() {
    setWorking(true);
    setError("");
    setMessage("");
    try {
      const card = await fetchHiveConnectionCard(operatorToken);
      downloadJson(card, `swarm-next-${safeFilename(card.payload.hive_name)}-connection.json`);
      setMessage("Connection card downloaded. It expires in 24 hours and grants no access by itself.");
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "The connection card could not be created.");
    } finally {
      setWorking(false);
    }
  }

  async function returnToPersonalHive() {
    setWorking(true);
    setError("");
    setMessage("");
    try {
      await collapseApiary(operatorToken);
      await refreshIdentity();
      setConfirmCollapse(false);
      setMessage("This installation is a personal Hive again.");
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "The Apiary could not be collapsed.");
      try {
        setReadiness(await fetchApiaryCollapseReadiness(operatorToken));
      } catch {
        setReadiness(undefined);
      }
    } finally {
      setWorking(false);
    }
  }

  async function promoteProject(binding: JiraProjectBinding) {
    setWorking(true);
    setError("");
    setMessage("");
    try {
      const project = await promoteApiaryJiraProject(operatorToken, binding.id);
      const [projects, bindings] = await Promise.all([
        fetchApiaryJiraProjects(operatorToken),
        fetchJiraBindings(operatorToken),
      ]);
      setPromotedProjects(projects);
      setJiraBindings(bindings);
      setMessage(`${project.project_key} is now in the Apiary project catalog.`);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "The Jira project could not be promoted.");
    } finally {
      setWorking(false);
    }
  }

  async function importConnectionCard(file: File | undefined) {
    if (!file) return;
    setWorking(true);
    setError("");
    setMessage("");
    try {
      if (file.size > 64 * 1024) throw new Error("That connection card is unexpectedly large.");
      const card = JSON.parse(await file.text()) as HiveConnectionCard;
      const candidate = await pinApiaryHiveCandidate(operatorToken, card);
      setHiveCandidates(await fetchApiaryHiveCandidates(operatorToken));
      setMessage(`${candidate.hive_name} is verified and pinned. No membership or access was granted.`);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "That Hive connection card could not be verified.");
    } finally {
      setWorking(false);
    }
  }

  async function createInvitation(candidate: ApiaryHiveCandidate) {
    setWorking(true);
    setError("");
    setMessage("");
    try {
      const bundle = await inviteApiaryHiveCandidate(operatorToken, candidate.hive_id);
      downloadJson(bundle, `swarm-next-${safeFilename(candidate.hive_name)}-invitation.json`);
      setHiveCandidates(await fetchApiaryHiveCandidates(operatorToken));
      setReadiness(await fetchApiaryCollapseReadiness(operatorToken));
      setMessage(`Invitation for ${candidate.hive_name} downloaded. This is the only copy of its one-time secret; share it privately.`);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "The one-time invitation could not be created.");
    } finally {
      setWorking(false);
    }
  }

  async function previewJoinInvitation(file: File | undefined) {
    if (!file) return;
    setError("");
    setMessage("");
    try {
      if (file.size > 128 * 1024) throw new Error("That invitation file is unexpectedly large.");
      const bundle = JSON.parse(await file.text()) as ApiaryInvitationBundle;
      if (!bundle?.keeper_connection_card?.payload || !bundle?.invitation?.payload || !bundle.one_time_secret) {
        throw new Error("That file is not a Swarm Apiary invitation.");
      }
      setInvitationPreview(bundle);
    } catch (cause) {
      setInvitationPreview(undefined);
      setError(cause instanceof Error ? cause.message : "That Apiary invitation could not be read.");
    }
  }

  async function trustKeeperAndImport() {
    if (!invitationPreview) return;
    setWorking(true);
    setError("");
    setMessage("");
    try {
      const imported = await importFederationJoinInvitation(operatorToken, invitationPreview);
      setJoinInvitations(await fetchFederationJoinInvitations(operatorToken));
      setInvitationPreview(undefined);
      setMessage(`${imported.keeper_hive_name} is pinned as Keeper for ${imported.apiary_name}. You have not joined or accepted its policy yet.`);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "That Apiary invitation could not be verified.");
    } finally {
      setWorking(false);
    }
  }

  return (
    <section id="settings-apiary" className="settings-card apiary-settings" aria-labelledby="apiary-heading">
      <div><p className="eyebrow">Collaboration</p><h3 id="apiary-heading">Your Apiary</h3></div>
      {personal ? (
        <>
          <p>Your personal Hive remains fully independent. Form an Apiary only when separate one-operator Hives should share Jira work and coordination.</p>
          <div className="apiary-connection-card">
            <span><strong>Share this Hive with a Keeper</strong><small>Download a signed, short-lived identity card. It contains no repositories, tasks, terminals, Jira access, or credentials.</small></span>
            <button className="secondary-button" disabled={busy || working} onClick={() => void downloadConnectionCard()}>Download connection card</button>
          </div>
          <div className="apiary-join-card">
            <div>
              <strong>Join an Apiary</strong>
              <small>Choose the invitation returned by a Keeper. Swarm shows the exact identity and policy revision before anything is pinned.</small>
            </div>
            <label className="apiary-card-drop" onDragOver={(event) => event.preventDefault()} onDrop={(event) => { event.preventDefault(); void previewJoinInvitation(event.dataTransfer.files[0]); }}>
              <input
                aria-label="Choose Apiary invitation"
                type="file"
                accept="application/json,.json"
                disabled={busy || working}
                onChange={(event) => { void previewJoinInvitation(event.target.files?.[0]); event.currentTarget.value = ""; }}
              />
              <span>Choose invitation</span>
              <small>or drop it here</small>
            </label>
            {invitationPreview ? (
              <div className="apiary-invitation-preview" role="group" aria-label="Review Apiary invitation">
                <div><span>Apiary</span><strong>{invitationPreview.invitation.payload.apiary_name}</strong></div>
                <div><span>Keeper Hive</span><strong>{invitationPreview.keeper_connection_card.payload.hive_name}</strong></div>
                <div><span>Keeper operator</span><strong>{invitationPreview.keeper_connection_card.payload.operator_display_name}</strong></div>
                <div><span>Shared work</span><strong>{invitationPreview.invitation.payload.shared_work_backend === "jira" ? "Jira-backed" : "Native Swarm"}</strong></div>
                <div><span>Policy revision</span><strong>{invitationPreview.invitation.payload.required_policy_revision}</strong></div>
                <div><span>Expires</span><strong>{new Date(invitationPreview.invitation.payload.expires_at * 1000).toLocaleString()}</strong></div>
                <p>Trusting pins this exact Keeper key and saves the one-time invitation privately. It does not join the Apiary, accept policy, share work, or grant terminal access.</p>
                <div className="settings-actions">
                  <button className="secondary-button" disabled={working} onClick={() => setInvitationPreview(undefined)}>Choose another</button>
                  <button className="primary-action" disabled={working} onClick={() => void trustKeeperAndImport()}>{working ? "Verifying…" : "Trust Keeper and save invitation"}</button>
                </div>
              </div>
            ) : null}
            {joinInvitations.length > 0 ? (
              <ul className="apiary-join-list" aria-label="Saved Apiary invitations">
                {joinInvitations.map((invitation) => (
                  <li key={invitation.invitation_id}>
                    <span><strong>{invitation.apiary_name}</strong><small>{invitation.keeper_hive_name} · {invitation.keeper_operator_display_name}</small></span>
                    <span className="readiness-ready">Keeper pinned · policy not accepted</span>
                  </li>
                ))}
              </ul>
            ) : <p className="empty-copy">No Apiary invitation is saved on this Hive.</p>}
          </div>
          <label className="field-stack" htmlFor="apiary-name">
            <span>Apiary name</span>
            <input id="apiary-name" value={name} maxLength={120} placeholder="Wildflower Garden" onChange={(event) => { setName(event.target.value); setConfirmCreate(false); }} />
          </label>
          <div className="apiary-backend-choice" aria-label="Shared work backend">
            <div className="selected"><strong>Jira-backed</strong><small>Available now · Jira remains canonical across every Hive.</small></div>
            <div aria-disabled="true"><strong>Native Swarm</strong><small>Later · requires distributed claims, offline queues, and reconciliation.</small></div>
          </div>
          {!confirmCreate ? (
            <button className="primary-action" disabled={busy || working || !name.trim()} onClick={() => setConfirmCreate(true)}>Review Apiary setup</button>
          ) : (
            <div className="apiary-confirmation" role="group" aria-label="Confirm Apiary setup">
              <strong>Use Jira as the permanent shared-work backend?</strong>
              <span>All active Hives will need their own Jira connection and access to every promoted project. The backend cannot be converted later.</span>
              <div className="settings-actions">
                <button className="secondary-button" disabled={working} onClick={() => setConfirmCreate(false)}>Go back</button>
                <button className="primary-action" disabled={working} onClick={() => void foundApiary()}>{working ? "Creating…" : "Found Jira-backed Apiary"}</button>
              </div>
            </div>
          )}
        </>
      ) : (
        <>
          <div className="apiary-summary">
            <span><strong>{context.apiary.name}</strong><small>{context.local_role === "keeper" ? "You are the Keeper" : "Member Hive"}</small></span>
            <span className="apiary-backend-badge">{context.apiary.shared_work_backend === "jira" ? "Jira-backed" : "Native"}</span>
          </div>
          <p>Workers, repositories, provider sessions, credentials, and private tasks remain owned by this Hive.</p>
          {keeper ? (
            <>
            <div className="apiary-hive-candidates">
              <div>
                <strong>Add a Hive</strong>
                <small>Choose the signed connection card from another personal Hive. Swarm verifies and pins its identity first; invitations and access remain separate.</small>
              </div>
              <label className="apiary-card-drop" onDragOver={(event) => event.preventDefault()} onDrop={(event) => { event.preventDefault(); void importConnectionCard(event.dataTransfer.files[0]); }}>
                <input
                  aria-label="Choose Hive connection card"
                  type="file"
                  accept="application/json,.json"
                  disabled={busy || working}
                  onChange={(event) => { void importConnectionCard(event.target.files?.[0]); event.currentTarget.value = ""; }}
                />
                <span>{working ? "Verifying…" : "Choose connection card"}</span>
                <small>or drop it here</small>
              </label>
              {candidateLoadError ? <p className="apiary-blockers">Pinned Hive identities could not be refreshed. No membership changed.</p> : null}
              {hiveCandidates.length > 0 ? (
                <ul className="apiary-candidate-list" aria-label="Pinned Hive identities">
                  {hiveCandidates.map((candidate) => (
                    <li key={candidate.hive_id}>
                      <span><strong>{candidate.hive_name}</strong><small>{candidate.operator_display_name}</small></span>
                      <span className="apiary-candidate-state">
                        <span className="readiness-ready">{candidate.invitation_pending ? "Invitation pending" : "Identity pinned"}</span>
                        <button
                          className="secondary-button"
                          disabled={working || candidate.invitation_pending}
                          onClick={() => void createInvitation(candidate)}
                        >{candidate.invitation_pending ? "Invitation created" : "Create invitation"}</button>
                      </span>
                    </li>
                  ))}
                </ul>
              ) : <p className="empty-copy">No other Hive identities are pinned yet.</p>}
            </div>
            <div className="apiary-projects">
              <div><strong>Apiary Jira projects</strong><small>Promote projects into the authoritative Apiary catalog. Each Hive must still receive the catalog entry and prove its own access and workflow mapping.</small></div>
              {promotedProjects.length > 0 ? (
                <ul className="apiary-project-list" aria-label="Promoted Jira projects">
                  {promotedProjects.map((project) => (
                    <li key={project.project_id}><strong>{project.project_key}</strong><span>{project.project_name}</span><small>Apiary catalog</small></li>
                  ))}
                </ul>
              ) : <p className="empty-copy">No projects are in the Apiary catalog yet.</p>}
              {projectLoadError ? <p className="apiary-blockers">The project catalog could not be fully refreshed. Existing Hive work is unchanged.</p> : null}
              {jiraBindings.some((binding) => binding.scope === "hive") ? (
                <div className="apiary-promotion-list" aria-label="Hive Jira projects available for promotion">
                  {jiraBindings.filter((binding) => binding.scope === "hive").map((binding) => {
                    const ready = binding.access_verified && binding.workflow_mapped;
                    return (
                      <div key={binding.id}>
                        <span><strong>{binding.project_key}</strong><small>{binding.project_name}</small></span>
                        <span className={ready ? "readiness-ready" : "readiness-blocked"}>{ready ? "Ready to promote" : "Finish access and workflow mapping"}</span>
                        <button className="secondary-button" disabled={working || !ready} onClick={() => void promoteProject(binding)}>Promote project</button>
                      </div>
                    );
                  })}
                </div>
              ) : null}
            </div>
            <div className="apiary-collapse">
              <div><strong>Return to a personal Hive</strong><small>Available only while this is the sole Hive and no federation state remains.</small></div>
              {readiness ? (
                <dl className="apiary-readiness">
                  <div><dt>Active Hives</dt><dd>{readiness.active_hive_count}</dd></div>
                  <div><dt>Invitations</dt><dd>{readiness.pending_invitation_count}</dd></div>
                  <div><dt>Stewardships</dt><dd>{readiness.active_stewardship_count}</dd></div>
                  <div><dt>Cross-Hive work</dt><dd>{readiness.open_cross_hive_work_count}</dd></div>
                  <div><dt>Departed nodes</dt><dd>{readiness.departed_node_count}</dd></div>
                </dl>
              ) : <small>Checking durable federation state…</small>}
              {blockers.length > 0 ? <p className="apiary-blockers">Clear before returning: {blockers.join(", ")}.</p> : null}
              {!confirmCollapse ? (
                <button className="secondary-button" disabled={busy || working || !readiness || blockers.length > 0} onClick={() => setConfirmCollapse(true)}>Review return to personal Hive</button>
              ) : (
                <div className="apiary-confirmation" role="group" aria-label="Confirm return to personal Hive">
                  <strong>Collapse {context.apiary.name}?</strong>
                  <span>Apiary Jira projects become Hive-owned. The inactive Apiary identity and lifecycle audit remain preserved.</span>
                  <div className="settings-actions">
                    <button className="secondary-button" disabled={working} onClick={() => setConfirmCollapse(false)}>Keep Apiary</button>
                    <button className="danger-button" disabled={working} onClick={() => void returnToPersonalHive()}>{working ? "Returning…" : "Return to personal Hive"}</button>
                  </div>
                </div>
              )}
            </div>
            </>
          ) : <small className="privacy-note">Membership changes require the Keeper and the explicit leave workflow. Nothing moves between Apiaries automatically.</small>}
        </>
      )}
      {message ? <p className="form-message" role="status">{message}</p> : null}
      {error ? <p className="form-error" role="alert">{error}</p> : null}
    </section>
  );
}

function collapseBlockers(readiness: ApiaryCollapseReadiness | undefined) {
  if (!readiness) return [];
  const blockers: string[] = [];
  if (readiness.active_hive_count !== 1) blockers.push(`${readiness.active_hive_count} active Hives`);
  if (readiness.pending_invitation_count) blockers.push(`${readiness.pending_invitation_count} invitation${readiness.pending_invitation_count === 1 ? "" : "s"}`);
  if (readiness.active_stewardship_count) blockers.push(`${readiness.active_stewardship_count} Stewardship${readiness.active_stewardship_count === 1 ? "" : "s"}`);
  if (readiness.open_cross_hive_work_count) blockers.push(`${readiness.open_cross_hive_work_count} cross-Hive work item${readiness.open_cross_hive_work_count === 1 ? "" : "s"}`);
  if (readiness.departed_node_count) blockers.push(`${readiness.departed_node_count} departed node${readiness.departed_node_count === 1 ? "" : "s"}`);
  return blockers;
}

function safeFilename(value: string) {
  return value.trim().toLocaleLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "") || "hive";
}

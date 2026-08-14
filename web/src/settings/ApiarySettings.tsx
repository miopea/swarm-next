import { useEffect, useMemo, useState } from "react";

import {
  acceptFederationJoinPolicy,
  collapseApiary,
  createApiary,
  fetchApiaryCollapseReadiness,
  fetchApiaryHiveCandidates,
  fetchApiaryJiraProjects,
  fetchApiaryMembers,
  fetchApiarySharedWork,
  fetchApiaryStewardships,
  fetchFederationCatalogReadiness,
  fetchFederationJoinInvitations,
  fetchFederationSyncHealth,
  fetchHive,
  fetchHiveConnectionCard,
  fetchJiraBindings,
  importFederationJoinInvitation,
  inviteApiaryHiveCandidate,
  pinApiaryHiveCandidate,
  prepareFederationJoin,
  promoteApiaryJiraProject,
  revokeApiaryStewardship,
  setApiaryStewardship,
  type ApiaryCollapseReadiness,
  type ApiaryHiveCandidate,
  type ApiaryInvitationBundle,
  type ApiaryJiraProject,
  type ApiaryMember,
  type ApiarySharedWorkClaim,
  type FederationJoinInvitationOverview,
  type FederationCatalogReadiness,
  type FederationSyncHealth,
  type HiveConnectionCard,
  type HiveIdentity,
  type JiraProjectBinding,
  type StewardCapability,
  type Stewardship,
} from "../api";
import { downloadJson } from "../shared/download";

const syncCopy = {
  idle: ["Not connected yet", "Automatic Keeper sync is not enabled in this build."],
  current: ["Up to date", "This Hive completed its latest Keeper reconciliation."],
  offline: ["Keeper temporarily unavailable", "Owned work remains local; new shared claims wait."],
  authentication_required: ["Membership credentials need attention", "Keeper synchronization is paused until access is restored."],
  incompatible: ["Runtime update required", "This Hive and its Keeper need compatible federation versions."],
} as const;

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
  const [members, setMembers] = useState<ApiaryMember[]>([]);
  const [sharedWork, setSharedWork] = useState<ApiarySharedWorkClaim[]>([]);
  const [stewardships, setStewardships] = useState<Stewardship[]>([]);
  const [memberSync, setMemberSync] = useState<FederationSyncHealth>();
  const [memberCatalog, setMemberCatalog] = useState<FederationCatalogReadiness>();
  const [memberSyncLoadError, setMemberSyncLoadError] = useState(false);
  const [hiveCandidates, setHiveCandidates] = useState<ApiaryHiveCandidate[]>([]);
  const [joinInvitations, setJoinInvitations] = useState<FederationJoinInvitationOverview[]>([]);
  const [invitationPreview, setInvitationPreview] = useState<ApiaryInvitationBundle>();
  const [jiraBindings, setJiraBindings] = useState<JiraProjectBinding[]>([]);
  const [projectLoadError, setProjectLoadError] = useState(false);
  const [candidateLoadError, setCandidateLoadError] = useState(false);
  const [sharedWorkLoadError, setSharedWorkLoadError] = useState(false);
  const [stewardshipLoadError, setStewardshipLoadError] = useState(false);
  const [editingSteward, setEditingSteward] = useState<string>();
  const [managedHives, setManagedHives] = useState<string[]>([]);
  const [stewardCapabilities, setStewardCapabilities] = useState<StewardCapability[]>([]);
  const [confirmRevoke, setConfirmRevoke] = useState<string>();
  const [working, setWorking] = useState(false);
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");
  const keeper = context?.mode === "federated" && context.local_role === "keeper";
  const member = context?.mode === "federated" && context.local_role === "member";

  useEffect(() => {
    if (personal) {
      setMembers([]);
      return;
    }
    let cancelled = false;
    void fetchApiaryMembers(operatorToken)
      .then((value) => { if (!cancelled) setMembers(value); })
      .catch(() => { if (!cancelled) setMembers([]); });
    return () => { cancelled = true; };
  }, [personal, operatorToken, hiveIdentity?.hive.apiary_id]);

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
    if (!member) {
      setMemberSync(undefined);
      setMemberCatalog(undefined);
      setMemberSyncLoadError(false);
      return;
    }
    let cancelled = false;
    void Promise.allSettled([
      fetchFederationSyncHealth(operatorToken),
      fetchFederationCatalogReadiness(operatorToken),
    ]).then(([sync, catalog]) => {
      if (cancelled) return;
      setMemberSync(sync.status === "fulfilled" ? sync.value : undefined);
      setMemberCatalog(catalog.status === "fulfilled" ? catalog.value : undefined);
      setMemberSyncLoadError(sync.status === "rejected" || catalog.status === "rejected");
    });
    return () => { cancelled = true; };
  }, [member, operatorToken, hiveIdentity?.hive.apiary_id]);

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
      setSharedWork([]);
      setSharedWorkLoadError(false);
      setStewardships([]);
      setStewardshipLoadError(false);
      setEditingSteward(undefined);
      return;
    }
    let cancelled = false;
    void Promise.allSettled([
      fetchApiaryJiraProjects(operatorToken),
      fetchJiraBindings(operatorToken),
      fetchApiaryHiveCandidates(operatorToken),
      fetchApiarySharedWork(operatorToken),
      fetchApiaryStewardships(operatorToken),
    ])
      .then(([projects, bindings, candidates, claims, delegations]) => {
        if (cancelled) return;
        setPromotedProjects(projects.status === "fulfilled" ? projects.value : []);
        setJiraBindings(bindings.status === "fulfilled" ? bindings.value : []);
        setHiveCandidates(candidates.status === "fulfilled" ? candidates.value : []);
        setProjectLoadError(projects.status === "rejected" || bindings.status === "rejected");
        setCandidateLoadError(candidates.status === "rejected");
        setSharedWork(claims.status === "fulfilled" ? claims.value : []);
        setSharedWorkLoadError(claims.status === "rejected");
        setStewardships(delegations.status === "fulfilled" ? delegations.value : []);
        setStewardshipLoadError(delegations.status === "rejected");
      });
    return () => { cancelled = true; };
  }, [keeper, operatorToken, hiveIdentity?.hive.apiary_id]);

  const blockers = useMemo(() => collapseBlockers(readiness), [readiness]);

  async function refreshIdentity() {
    const identity = await fetchHive(operatorToken);
    onHiveIdentityChange(identity);
  }

  function editSteward(operatorId: string) {
    const existing = stewardships.find((stewardship) => stewardship.steward_operator_id === operatorId);
    const stewardMember = members.find((member) => member.operator_id === operatorId);
    setEditingSteward(operatorId);
    setManagedHives(existing?.managed_hive_ids ?? (stewardMember ? [stewardMember.hive_id] : []));
    setStewardCapabilities(existing?.capabilities ?? ["observe", "assign", "assist", "takeover"]);
    setConfirmRevoke(undefined);
    setError("");
    setMessage("");
  }

  async function saveStewardship() {
    if (!editingSteward || managedHives.length === 0) return;
    setWorking(true);
    setError("");
    setMessage("");
    try {
      const capabilities = stewardCapabilities.includes("observe")
        ? stewardCapabilities
        : ["observe" as const, ...stewardCapabilities];
      const saved = await setApiaryStewardship(
        operatorToken,
        editingSteward,
        managedHives,
        capabilities,
      );
      setStewardships((current) => [
        ...current.filter((item) => item.steward_operator_id !== saved.steward_operator_id),
        saved,
      ]);
      const operator = members.find((member) => member.operator_id === saved.steward_operator_id);
      setMessage(`${operator?.operator_display_name ?? "The member"} is now a Steward for ${saved.managed_hive_ids.length} ${saved.managed_hive_ids.length === 1 ? "Hive" : "Hives"}.`);
      setEditingSteward(undefined);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "The Steward delegation could not be saved.");
    } finally {
      setWorking(false);
    }
  }

  async function revokeStewardship(stewardship: Stewardship) {
    setWorking(true);
    setError("");
    setMessage("");
    try {
      await revokeApiaryStewardship(operatorToken, stewardship.id);
      setStewardships((current) => current.filter((item) => item.id !== stewardship.id));
      const operator = members.find((member) => member.operator_id === stewardship.steward_operator_id);
      setMessage(`${operator?.operator_display_name ?? "The member"} is no longer a Steward.`);
      setConfirmRevoke(undefined);
      if (editingSteward === stewardship.steward_operator_id) setEditingSteward(undefined);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "The Steward delegation could not be revoked.");
    } finally {
      setWorking(false);
    }
  }

  function toggleManagedHive(hiveId: string) {
    setManagedHives((current) => current.includes(hiveId)
      ? current.filter((id) => id !== hiveId)
      : [...current, hiveId]);
  }

  function toggleStewardCapability(capability: StewardCapability) {
    setStewardCapabilities((current) => current.includes(capability)
      ? current.filter((item) => item !== capability)
      : [...current, capability]);
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
      if (file.size > 512 * 1024) throw new Error("That invitation file is unexpectedly large.");
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

  async function acceptImportedPolicy(invitation: FederationJoinInvitationOverview) {
    setWorking(true);
    setError("");
    setMessage("");
    try {
      await acceptFederationJoinPolicy(
        operatorToken,
        invitation.invitation_id,
        invitation.required_policy_revision,
      );
      setJoinInvitations(await fetchFederationJoinInvitations(operatorToken));
      setMessage(`Policy revision ${invitation.required_policy_revision} accepted locally. This Hive has not joined ${invitation.apiary_name} yet.`);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "That policy revision could not be accepted.");
    } finally {
      setWorking(false);
    }
  }

  async function prepareJoinRequest(invitation: FederationJoinInvitationOverview) {
    setWorking(true);
    setError("");
    setMessage("");
    try {
      await prepareFederationJoin(operatorToken, invitation.invitation_id);
      setJoinInvitations(await fetchFederationJoinInvitations(operatorToken));
      setMessage(`The signed join request for ${invitation.apiary_name} is prepared locally. Nothing has been sent to the Keeper yet.`);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "The join request could not be prepared.");
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
                <div><span>Shared Jira projects</span><strong>{invitationPreview.promoted_projects.length}</strong></div>
                <div><span>Expires</span><strong>{new Date(invitationPreview.invitation.payload.expires_at * 1000).toLocaleString()}</strong></div>
                {invitationPreview.promoted_projects.length > 0 ? (
                  <ul className="apiary-project-manifest" aria-label="Promoted Jira projects">
                    {invitationPreview.promoted_projects.map((project) => (
                      <li key={project.project_id}><strong>{project.project_key}</strong><span>{project.project_name}</span></li>
                    ))}
                  </ul>
                ) : <p className="empty-copy">This Apiary has no promoted Jira projects yet.</p>}
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
                    <div className="apiary-join-summary">
                      <span><strong>{invitation.apiary_name}</strong><small>{invitation.keeper_hive_name} · {invitation.keeper_operator_display_name}</small></span>
                      <span className={invitation.readiness.blockers.length === 0 ? "readiness-ready" : "readiness-blocked"}>
                        {invitation.readiness_compatibility_fallback
                          ? "Runtime update in progress"
                          : invitation.readiness.blockers.length === 0
                            ? "Ready to contact Keeper"
                            : `${invitation.readiness.blockers.length} readiness ${invitation.readiness.blockers.length === 1 ? "step" : "steps"} left`}
                      </span>
                    </div>
                    <div className="apiary-policy-acknowledgement">
                      <span>
                        <strong>Policy revision {invitation.required_policy_revision}</strong>
                        <small>Jira-backed shared work · {invitation.promoted_projects.length} signed {invitation.promoted_projects.length === 1 ? "project" : "projects"} · Keeper identity pinned</small>
                      </span>
                      {invitation.readiness_compatibility_fallback ? (
                        <button className="secondary-button" disabled>Waiting for runtime</button>
                      ) : invitation.state === "submitted" ? (
                        <span className="readiness-ready">Join request prepared</span>
                      ) : invitation.state === "keeper_pinned" ? (
                        <button className="secondary-button" disabled={working} onClick={() => void acceptImportedPolicy(invitation)}>
                          Acknowledge revision {invitation.required_policy_revision}
                        </button>
                      ) : invitation.readiness.blockers.length === 0 ? (
                        <button className="primary-action" disabled={working} onClick={() => void prepareJoinRequest(invitation)}>
                          {working ? "Preparing…" : "Prepare join request"}
                        </button>
                      ) : <span className="readiness-ready">Acknowledged</span>}
                    </div>
                    <ul className="apiary-project-readiness" aria-label={`Jira readiness for ${invitation.apiary_name}`}>
                      {invitation.readiness.projects.map((project) => {
                        const ready = project.binding_id && project.access_verified && project.workflow_mapped;
                        const status = invitation.readiness_compatibility_fallback
                          ? "Readiness refresh pending"
                          : !project.binding_id
                          ? "Connect this Jira project"
                          : !project.access_verified
                            ? "Verify Jira access"
                            : !project.workflow_mapped
                              ? "Finish workflow mapping"
                              : "Connected and mapped";
                        return (
                          <li key={project.project.project_id}>
                            <span><strong>{project.project.project_key}</strong><small>{project.project.project_name}</small></span>
                            <span className={ready ? "readiness-ready" : "readiness-blocked"}>{status}</span>
                          </li>
                        );
                      })}
                    </ul>
                    {invitation.readiness.jira_connection !== "ready" ? (
                      <p className="readiness-blocked">Connect Jira on this Hive before it can join.</p>
                    ) : null}
                    <small>{invitation.state === "submitted" ? "The signed request is durable and retry-stable. Delivery to the Keeper is not enabled yet." : "Nothing is sent to the Keeper and no membership is granted at this step."}</small>
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
          {member ? (
            <div className="apiary-member-sync" aria-label="Keeper synchronization status">
              <div>
                <span className={`apiary-sync-indicator apiary-sync-${memberSync?.condition ?? "idle"}`} aria-hidden="true" />
                <span>
                  <strong>{syncCopy[memberSync?.condition ?? "idle"][0]}</strong>
                  <small>{syncCopy[memberSync?.condition ?? "idle"][1]}</small>
                </span>
              </div>
              <dl>
                <div><dt>Catalog</dt><dd>{memberCatalog?.acknowledgement ? "Verified" : "Waiting"}</dd></div>
                <div><dt>Projects ready</dt><dd>{memberCatalog ? `${memberCatalog.projects.filter((project) => project.binding_id && project.access_verified && project.workflow_mapped).length}/${memberCatalog.projects.length}` : "—"}</dd></div>
                <div><dt>Jira</dt><dd>{memberCatalog?.jira_connection === "ready" ? "Connected" : "Needs attention"}</dd></div>
                <div><dt>Retries</dt><dd>{memberSync?.consecutive_failures ?? 0}</dd></div>
              </dl>
              {memberCatalog && memberCatalog.blockers.length > 0 ? (
                <p className="apiary-blockers">Shared work waits for: {memberCatalog.blockers.map((blocker) => blocker.replaceAll("_", " ")).join(", ")}.</p>
              ) : null}
              {memberSyncLoadError ? <p className="apiary-blockers">Synchronization status could not be refreshed. Local workers and owned work are unchanged.</p> : null}
            </div>
          ) : null}
          <div className="apiary-members">
            <div><strong>Hives in this Apiary</strong><small>Registered membership, not live presence. Each Hive remains independently operated.</small></div>
            {members.length > 0 ? (
              <ul aria-label="Apiary Hives">
                {members.map((member) => (
                  <li key={member.hive_id}>
                    <span><strong>{member.hive_name}</strong><small>{member.operator_display_name}</small></span>
                    <span className="apiary-member-badges">
                      <span>{member.role === "keeper" ? "Keeper" : "Member"}</span>
                      {member.is_local ? <span className="readiness-ready">This Hive</span> : null}
                    </span>
                  </li>
                ))}
              </ul>
            ) : <p className="empty-copy">Membership is being refreshed.</p>}
          </div>
          {keeper ? (
            <>
            <div className="apiary-stewards">
              <div className="apiary-section-heading">
                <span>
                  <strong>Stewards</strong>
                  <small>Give a trusted team lead durable authority over selected Hives. Routine work stays quiet; assistance and takeover remain visible and audited.</small>
                </span>
                {!editingSteward && members.some((item) => item.role === "member") ? (
                  <button
                    className="secondary-button"
                    disabled={working}
                    onClick={() => editSteward(members.find((item) => item.role === "member")!.operator_id)}
                  >Delegate a Steward</button>
                ) : null}
              </div>
              {stewardshipLoadError ? <p className="apiary-blockers">Steward delegations could not be refreshed. Existing authority is unchanged.</p> : null}
              {stewardships.length > 0 ? (
                <ul className="apiary-steward-list" aria-label="Apiary Stewards">
                  {stewardships.map((stewardship) => {
                    const steward = members.find((item) => item.operator_id === stewardship.steward_operator_id);
                    const managedNames = stewardship.managed_hive_ids.map((hiveId) => members.find((item) => item.hive_id === hiveId)?.hive_name ?? "Unknown Hive");
                    return (
                      <li key={stewardship.id}>
                        <span>
                          <strong>{steward?.operator_display_name ?? "Apiary member"}</strong>
                          <small>{steward?.hive_name ?? "Member Hive"}</small>
                        </span>
                        <span className="apiary-steward-scope">
                          <small>Responsible for</small>
                          <strong>{managedNames.join(", ")}</strong>
                        </span>
                        <span className="apiary-steward-capabilities" aria-label="Granted capabilities">
                          {stewardship.capabilities.map((capability) => <span key={capability}>{stewardCapabilityLabel(capability)}</span>)}
                        </span>
                        <span className="apiary-steward-actions">
                          <button className="secondary-button" disabled={working} onClick={() => editSteward(stewardship.steward_operator_id)}>Edit</button>
                          {confirmRevoke === stewardship.id ? (
                            <span className="apiary-revoke-confirm">
                              <button className="secondary-button" disabled={working} onClick={() => setConfirmRevoke(undefined)}>Keep</button>
                              <button className="danger-button" disabled={working} onClick={() => void revokeStewardship(stewardship)}>Confirm revoke</button>
                            </span>
                          ) : (
                            <button className="secondary-button" disabled={working} onClick={() => setConfirmRevoke(stewardship.id)}>Revoke</button>
                          )}
                        </span>
                      </li>
                    );
                  })}
                </ul>
              ) : !editingSteward ? <p className="empty-copy">No Stewards are delegated. Member Hives escalate directly to you.</p> : null}
              {editingSteward ? (
                <div className="apiary-steward-editor" role="group" aria-label="Steward delegation">
                  <label className="field-stack" htmlFor="steward-operator">
                    <span>Team lead</span>
                    <select id="steward-operator" value={editingSteward} onChange={(event) => editSteward(event.target.value)}>
                      {members.filter((item) => item.role === "member").map((item) => (
                        <option key={item.operator_id} value={item.operator_id}>{item.operator_display_name} · {item.hive_name}</option>
                      ))}
                    </select>
                  </label>
                  <fieldset>
                    <legend>Hives this Steward can help</legend>
                    <small>Select the complete durable scope. Changes replace the previous grant atomically.</small>
                    <div className="apiary-choice-grid">
                      {members.filter((item) => item.role === "member").map((item) => (
                        <label key={item.hive_id}>
                          <input type="checkbox" checked={managedHives.includes(item.hive_id)} onChange={() => toggleManagedHive(item.hive_id)} />
                          <span><strong>{item.hive_name}</strong><small>{item.operator_display_name}</small></span>
                        </label>
                      ))}
                    </div>
                  </fieldset>
                  <fieldset>
                    <legend>What this Steward can do</legend>
                    <small>Viewing is always included. Takeover replaces active control visibly; it never injects into an operator’s session.</small>
                    <div className="apiary-choice-grid">
                      {stewardCapabilityChoices.map(({ capability, label, detail }) => (
                        <label key={capability}>
                          <input
                            type="checkbox"
                            checked={capability === "observe" || stewardCapabilities.includes(capability)}
                            disabled={capability === "observe"}
                            onChange={() => toggleStewardCapability(capability)}
                          />
                          <span><strong>{label}</strong><small>{detail}</small></span>
                        </label>
                      ))}
                    </div>
                  </fieldset>
                  <p className="privacy-note">This records authority on the Keeper now. Delivery to remote Hives follows federation synchronization.</p>
                  <div className="settings-actions">
                    <button className="secondary-button" disabled={working} onClick={() => setEditingSteward(undefined)}>Cancel</button>
                    <button className="primary-action" disabled={working || managedHives.length === 0} onClick={() => void saveStewardship()}>{working ? "Saving…" : "Save Stewardship"}</button>
                  </div>
                </div>
              ) : null}
            </div>
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
            <div className="apiary-shared-work">
              <div>
                <span><strong>Shared work ownership</strong><small>Only active reservations and durable home-Hive ownership appear here. Routine worker activity stays inside each Hive.</small></span>
                <span className="apiary-shared-work-count">{sharedWork.length}</span>
              </div>
              {sharedWorkLoadError ? (
                <p className="apiary-blockers">Shared ownership could not be refreshed. No claim or Jira state changed.</p>
              ) : sharedWork.length > 0 ? (
                <ul aria-label="Apiary shared work ownership">
                  {sharedWork.map((claim) => (
                    <li key={claim.id}>
                      <span className="apiary-claim-issue"><strong>{claim.issue_key}</strong><small>{claim.project_name}</small></span>
                      <span><strong>{claim.home_hive_name}</strong><small>{claim.home_operator_display_name}</small></span>
                      <span className={claim.state === "confirmed" ? "readiness-ready" : "apiary-claim-pending"}>
                        {claim.state === "confirmed" ? "Owned" : "Claiming"}
                      </span>
                    </li>
                  ))}
                </ul>
              ) : <p className="empty-copy">No shared Jira work is currently claimed by an Apiary Hive.</p>}
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

const stewardCapabilityChoices: { capability: StewardCapability; label: string; detail: string }[] = [
  { capability: "observe", label: "See status", detail: "Structured Hive and work state" },
  { capability: "assign", label: "Route work", detail: "Assign shared work inside the selected Hives" },
  { capability: "assist", label: "Assist", detail: "Offer scoped help when attention is needed" },
  { capability: "takeover", label: "Take over", detail: "Explicitly replace an engagement lease" },
  { capability: "manage_projects", label: "Manage projects", detail: "Maintain shared project configuration" },
  { capability: "manage_members", label: "Manage members", detail: "Administer approved Hive membership" },
];

function stewardCapabilityLabel(capability: StewardCapability) {
  return stewardCapabilityChoices.find((item) => item.capability === capability)?.label ?? capability;
}

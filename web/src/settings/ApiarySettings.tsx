import { useEffect, useMemo, useState } from "react";

import {
  collapseApiary,
  createApiary,
  fetchApiaryCollapseReadiness,
  fetchApiaryJiraProjects,
  fetchApiaryMembers,
  fetchApiarySharedWork,
  fetchApiaryStewardships,
  fetchFederationCatalogReadiness,
  fetchFederationSyncHealth,
  fetchHive,
  fetchJiraBindings,
  promoteApiaryJiraProject,
  renameApiary,
  renameHive,
  revokeApiaryStewardship,
  setApiaryStewardship,
  type ApiaryCollapseReadiness,
  type ApiaryJiraProject,
  type ApiaryMember,
  type ApiarySharedWorkClaim,
  type FederationCatalogReadiness,
  type FederationSyncHealth,
  type HiveIdentity,
  type JiraProjectBinding,
  type LocalApiaryContext,
  type StewardCapability,
  type Stewardship,
} from "../api";
import { federationSyncCopy } from "../apiary/presentation";
import KeeperInvitationManager from "./KeeperInvitationManager";
import MemberDeparturePanel from "./MemberDeparturePanel";
import PersonalHiveJoin from "./PersonalHiveJoin";

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
  const [editingIdentity, setEditingIdentity] = useState(false);
  const [hiveName, setHiveName] = useState(hiveIdentity?.hive.name ?? "");
  const [apiaryName, setApiaryName] = useState(context?.mode === "federated" ? context.apiary.name : "");
  const [confirmCollapse, setConfirmCollapse] = useState(false);
  const [readiness, setReadiness] = useState<ApiaryCollapseReadiness>();
  const [promotedProjects, setPromotedProjects] = useState<ApiaryJiraProject[]>([]);
  const [members, setMembers] = useState<ApiaryMember[]>([]);
  const [memberRosterState, setMemberRosterState] = useState<"loading" | "ready" | "error">("loading");
  const [memberRosterAttempt, setMemberRosterAttempt] = useState(0);
  const [sharedWork, setSharedWork] = useState<ApiarySharedWorkClaim[]>([]);
  const [stewardships, setStewardships] = useState<Stewardship[]>([]);
  const [memberSync, setMemberSync] = useState<FederationSyncHealth>();
  const [memberCatalog, setMemberCatalog] = useState<FederationCatalogReadiness>();
  const [memberSyncLoadError, setMemberSyncLoadError] = useState(false);
  const [jiraBindings, setJiraBindings] = useState<JiraProjectBinding[]>([]);
  const [projectLoadError, setProjectLoadError] = useState(false);
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
    if (editingIdentity) return;
    setHiveName(hiveIdentity?.hive.name ?? "");
    setApiaryName(context?.mode === "federated" ? context.apiary.name : "");
  }, [editingIdentity, hiveIdentity?.hive.name, context]);

  useEffect(() => {
    if (personal) {
      setMembers([]);
      setMemberRosterState("ready");
      return;
    }
    let cancelled = false;
    setMemberRosterState("loading");
    void fetchApiaryMembers(operatorToken)
      .then((value) => {
        if (cancelled) return;
        setMembers(value);
        setMemberRosterState("ready");
      })
      .catch(() => {
        if (!cancelled) setMemberRosterState("error");
      });
    return () => { cancelled = true; };
  }, [personal, operatorToken, hiveIdentity?.hive.apiary_id, memberRosterAttempt]);

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
    if (!keeper) {
      setPromotedProjects([]);
      setJiraBindings([]);
      setProjectLoadError(false);
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
      fetchApiarySharedWork(operatorToken),
      fetchApiaryStewardships(operatorToken),
    ])
      .then(([projects, bindings, claims, delegations]) => {
        if (cancelled) return;
        if (projects.status === "fulfilled") setPromotedProjects(projects.value);
        if (bindings.status === "fulfilled") setJiraBindings(bindings.value);
        setProjectLoadError(projects.status === "rejected" || bindings.status === "rejected");
        if (claims.status === "fulfilled") setSharedWork(claims.value);
        setSharedWorkLoadError(claims.status === "rejected");
        if (delegations.status === "fulfilled") setStewardships(delegations.value);
        setStewardshipLoadError(delegations.status === "rejected");
      });
    return () => { cancelled = true; };
  }, [keeper, operatorToken, hiveIdentity?.hive.apiary_id]);

  const blockers = useMemo(() => collapseBlockers(readiness), [readiness]);

  async function refreshIdentity() {
    const identity = await fetchHive(operatorToken);
    onHiveIdentityChange(identity);
  }

  function applyApiaryContext(nextContext: LocalApiaryContext) {
    if (!hiveIdentity) return;
    onHiveIdentityChange({
      ...hiveIdentity,
      hive: {
        ...hiveIdentity.hive,
        apiary_id: nextContext.mode === "federated" ? nextContext.apiary.id : null,
      },
      apiary_context: nextContext,
    });
  }

  async function saveHiveName() {
    const nextName = hiveName.trim();
    if (!nextName) return;
    setWorking(true);
    setError("");
    setMessage("");
    try {
      const identity = await renameHive(operatorToken, nextName);
      onHiveIdentityChange(identity);
      setHiveName(identity.hive.name);
      setMessage(`This Hive is now named ${identity.hive.name}.`);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "The Hive name could not be saved.");
    } finally {
      setWorking(false);
    }
  }

  async function saveApiaryName() {
    const nextName = apiaryName.trim();
    if (!nextName || !keeper) return;
    setWorking(true);
    setError("");
    setMessage("");
    try {
      const nextContext = await renameApiary(operatorToken, nextName);
      applyApiaryContext(nextContext);
      const savedName = nextContext.mode === "federated" ? nextContext.apiary.name : nextName;
      setApiaryName(savedName);
      setMessage(`This Apiary is now named ${savedName}.`);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "The Apiary name could not be saved.");
    } finally {
      setWorking(false);
    }
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
      const nextContext = await createApiary(operatorToken, name.trim(), "jira");
      applyApiaryContext(nextContext);
      setConfirmCreate(false);
      const createdName = nextContext.mode === "federated" ? nextContext.apiary.name : name.trim();
      setMessage(`${createdName} is now a Jira-backed Apiary.`);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "The Apiary could not be created.");
    } finally {
      setWorking(false);
    }
  }

  async function returnToPersonalHive() {
    setWorking(true);
    setError("");
    setMessage("");
    try {
      const nextContext = await collapseApiary(operatorToken);
      applyApiaryContext(nextContext);
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
      setPromotedProjects((current) => [
        ...current.filter((item) => item.project_id !== project.project_id),
        project,
      ]);
      setJiraBindings((current) => current.map((item) => item.id === binding.id
        ? { ...item, scope: "apiary", apiary_id: project.apiary_id }
        : item));
      setMessage(`${project.project_key} is now in the Apiary project catalog.`);
      const [projects, bindings] = await Promise.allSettled([
        fetchApiaryJiraProjects(operatorToken),
        fetchJiraBindings(operatorToken),
      ]);
      if (projects.status === "fulfilled") setPromotedProjects(projects.value);
      if (bindings.status === "fulfilled") setJiraBindings(bindings.value);
      setProjectLoadError(projects.status === "rejected" || bindings.status === "rejected");
      if (projects.status === "rejected" || bindings.status === "rejected") {
        setError(`${project.project_key} was promoted successfully, but the project lists could not be fully refreshed. Do not promote it again; retry the project status instead.`);
      }
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "The Jira project could not be promoted.");
    } finally {
      setWorking(false);
    }
  }

  return (
    <section id="settings-apiary" className="settings-card apiary-settings" aria-labelledby="apiary-heading">
      <div><p className="eyebrow">Collaboration</p><h3 id="apiary-heading">Your Apiary</h3></div>
      {hiveIdentity ? (
        <div className="apiary-identity-panel">
          <div className="apiary-identity-summary">
            <span><small>This Hive</small><strong>{hiveIdentity.hive.name}</strong></span>
            {context?.mode === "federated" ? (
              <span><small>{context.local_role === "keeper" ? "Keeper of" : "Member of"}</small><strong>{context.apiary.name}</strong></span>
            ) : <span><small>Mode</small><strong>Personal Hive</strong></span>}
            {context?.mode === "federated" ? <span className="apiary-backend-badge">{context.apiary.shared_work_backend === "jira" ? "Jira-backed" : "Native"}</span> : null}
            <button className="secondary-button" disabled={working} onClick={() => setEditingIdentity((current) => !current)}>{editingIdentity ? "Close names" : "Edit names"}</button>
          </div>
          {editingIdentity ? (
            <div className="apiary-identity-editor" role="group" aria-label="Hive and Apiary names">
              <label className="field-stack" htmlFor="local-hive-name">
                <span>Hive name</span>
                <input id="local-hive-name" value={hiveName} maxLength={120} onChange={(event) => setHiveName(event.target.value)} />
              </label>
              <button className="secondary-button" disabled={working || !hiveName.trim() || hiveName.trim() === hiveIdentity.hive.name} onClick={() => void saveHiveName()}>Save Hive name</button>
              {context?.mode === "federated" ? (
                <>
                  <label className="field-stack" htmlFor="current-apiary-name">
                    <span>Apiary name</span>
                    <input id="current-apiary-name" value={apiaryName} maxLength={120} disabled={!keeper} onChange={(event) => setApiaryName(event.target.value)} />
                  </label>
                  {keeper ? (
                    <button className="secondary-button" disabled={working || !apiaryName.trim() || apiaryName.trim() === context.apiary.name} onClick={() => void saveApiaryName()}>Save Apiary name</button>
                  ) : <small>Only the Keeper can rename the Apiary.</small>}
                </>
              ) : null}
              <p className="privacy-note">Names update the public roster. Worker ownership, repositories, tasks, Jira projects, and federation keys do not change.</p>
            </div>
          ) : null}
        </div>
      ) : null}
      {personal ? (
        <>
          <p>Your personal Hive remains fully independent. Form an Apiary only when separate one-operator Hives should share Jira work and coordination.</p>
          <PersonalHiveJoin busy={busy || working} operatorToken={operatorToken} onMessage={setMessage} onError={setError} onJoined={refreshIdentity} />
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
          <p>Workers, repositories, provider sessions, credentials, and private tasks remain owned by this Hive.</p>
          {member ? (
            <div className="apiary-member-sync" aria-label="Keeper synchronization status">
              <div>
                <span className={`apiary-sync-indicator apiary-sync-${memberSync?.condition ?? "idle"}`} aria-hidden="true" />
                <span>
                  <strong>{federationSyncCopy[memberSync?.condition ?? "idle"][0]}</strong>
                  <small>{federationSyncCopy[memberSync?.condition ?? "idle"][1]}</small>
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
            ) : memberRosterState === "loading" ? <p className="empty-copy">Membership is being refreshed.</p> : null}
            {memberRosterState === "error" ? (
              <div className="apiary-blockers" role="alert">
                <p>The Hive roster could not be refreshed. Last-known membership remains unchanged.</p>
                <button className="secondary-button" type="button" onClick={() => setMemberRosterAttempt((attempt) => attempt + 1)}>Retry Hive roster</button>
              </div>
            ) : null}
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
            <KeeperInvitationManager
              busy={busy || working}
              operatorToken={operatorToken}
              onInvitationCreated={async () => setReadiness(await fetchApiaryCollapseReadiness(operatorToken))}
            />
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
          ) : context?.mode === "federated" ? (
            <MemberDeparturePanel
              apiaryName={context.apiary.name}
              busy={busy || working}
              operatorToken={operatorToken}
              onLeft={refreshIdentity}
            />
          ) : null}
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

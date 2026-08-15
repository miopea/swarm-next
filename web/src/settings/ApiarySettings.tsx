import { useEffect, useId, useMemo, useState, type ReactNode } from "react";

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
  fetchFederationTransportReadiness,
  fetchHive,
  fetchHiveConnectionCard,
  fetchJiraBindings,
  importFederationJoinInvitation,
  inviteApiaryHiveCandidate,
  pinApiaryHiveCandidate,
  prepareFederationJoin,
  promoteApiaryJiraProject,
  renameApiary,
  renameHive,
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
  type FederationTransportReadiness,
  type HiveConnectionCard,
  type HiveIdentity,
  type JiraProjectBinding,
  type StewardCapability,
  type Stewardship,
} from "../api";
import { createApiaryHandoffLink, readApiaryHandoffLink } from "./apiaryHandoff";

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
  const [editingIdentity, setEditingIdentity] = useState(false);
  const [hiveName, setHiveName] = useState(hiveIdentity?.hive.name ?? "");
  const [apiaryName, setApiaryName] = useState(context?.mode === "federated" ? context.apiary.name : "");
  const [confirmCollapse, setConfirmCollapse] = useState(false);
  const [readiness, setReadiness] = useState<ApiaryCollapseReadiness>();
  const [promotedProjects, setPromotedProjects] = useState<ApiaryJiraProject[]>([]);
  const [members, setMembers] = useState<ApiaryMember[]>([]);
  const [sharedWork, setSharedWork] = useState<ApiarySharedWorkClaim[]>([]);
  const [stewardships, setStewardships] = useState<Stewardship[]>([]);
  const [memberSync, setMemberSync] = useState<FederationSyncHealth>();
  const [transportReadiness, setTransportReadiness] = useState<FederationTransportReadiness>();
  const [memberCatalog, setMemberCatalog] = useState<FederationCatalogReadiness>();
  const [memberSyncLoadError, setMemberSyncLoadError] = useState(false);
  const [hiveCandidates, setHiveCandidates] = useState<ApiaryHiveCandidate[]>([]);
  const [joinInvitations, setJoinInvitations] = useState<FederationJoinInvitationOverview[]>([]);
  const [invitationPreview, setInvitationPreview] = useState<ApiaryInvitationBundle>();
  const [connectionLink, setConnectionLink] = useState("");
  const [invitationLink, setInvitationLink] = useState("");
  const [generatedHandoff, setGeneratedHandoff] = useState<{ kind: "connection" | "invitation"; link: string }>();
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
    if (editingIdentity) return;
    setHiveName(hiveIdentity?.hive.name ?? "");
    setApiaryName(context?.mode === "federated" ? context.apiary.name : "");
  }, [editingIdentity, hiveIdentity?.hive.name, context]);

  useEffect(() => {
    let cancelled = false;
    void fetchFederationTransportReadiness(operatorToken)
      .then((value) => { if (!cancelled) setTransportReadiness(value); })
      .catch(() => { if (!cancelled) setTransportReadiness(undefined); });
    return () => { cancelled = true; };
  }, [operatorToken]);

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
      await renameApiary(operatorToken, nextName);
      await refreshIdentity();
      setApiaryName(nextName);
      setMessage(`This Apiary is now named ${nextName}.`);
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

  async function copyHandoffLink(link: string): Promise<boolean> {
    try {
      await navigator.clipboard.writeText(link);
      return true;
    } catch {
      return false;
    }
  }

  async function createConnectionLink() {
    setWorking(true);
    setError("");
    setMessage("");
    try {
      const card = await fetchHiveConnectionCard(operatorToken);
      const link = createApiaryHandoffLink("connection", card);
      setGeneratedHandoff({ kind: "connection", link });
      const copied = await copyHandoffLink(link);
      setMessage(copied
        ? "Connection link copied. It expires in 24 hours and grants no access by itself."
        : "Connection link created. Copy it below; it expires in 24 hours and grants no access by itself.");
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "The connection link could not be created.");
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

  async function importConnectionLink() {
    setWorking(true);
    setError("");
    setMessage("");
    try {
      const card = readApiaryHandoffLink<HiveConnectionCard>(connectionLink, "connection");
      const candidate = await pinApiaryHiveCandidate(operatorToken, card);
      setHiveCandidates(await fetchApiaryHiveCandidates(operatorToken));
      setConnectionLink("");
      setMessage(`${candidate.hive_name} is verified and pinned. No membership or access was granted.`);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "That Hive connection link could not be verified.");
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
      const link = createApiaryHandoffLink("invitation", bundle);
      setGeneratedHandoff({ kind: "invitation", link });
      const copied = await copyHandoffLink(link);
      setHiveCandidates(await fetchApiaryHiveCandidates(operatorToken));
      setReadiness(await fetchApiaryCollapseReadiness(operatorToken));
      setMessage(copied
        ? `Invitation link for ${candidate.hive_name} copied. It is bound to that Hive, expires, and can be used only once.`
        : `Invitation link for ${candidate.hive_name} created. Copy it below and share it privately.`);
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

  function previewJoinInvitationLink() {
    setError("");
    setMessage("");
    try {
      const bundle = readApiaryHandoffLink<ApiaryInvitationBundle>(invitationLink, "invitation");
      if (!bundle?.keeper_connection_card?.payload || !bundle?.invitation?.payload || !bundle.one_time_secret) {
        throw new Error("That link is not a Swarm Apiary invitation.");
      }
      setInvitationPreview(bundle);
      setInvitationLink("");
    } catch (cause) {
      setInvitationPreview(undefined);
      setError(cause instanceof Error ? cause.message : "That Apiary invitation link could not be read.");
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
          <FederationTransportStatus readiness={transportReadiness} />
          <div className="apiary-exchange-intro">
            <span><strong>Join another Keeper's Apiary</strong><small>Three deliberate handoffs keep each operator in control. No membership or shared access changes until both Hives verify the exact identities.</small></span>
            <ol className="apiary-exchange-guide" aria-label="How to join an Apiary">
              <ApiaryExchangeStep number="1" title="Copy this Hive's connection link" detail="Send the short-lived link privately to the Keeper. Its signed identity contains no repositories, tasks, terminals, Jira access, credentials, or invitation secret.">
                <button className="secondary-button" disabled={busy || working} onClick={() => void createConnectionLink()}>Copy connection link</button>
              </ApiaryExchangeStep>
              <ApiaryExchangeStep number="2" title="Keeper verifies and invites" detail="The Keeper pastes the link, checks your Hive and operator names, then returns one invitation link bound only to this Hive." />
              <ApiaryExchangeStep number="3" title="Review before joining" detail="Paste the returned link below. Swarm verifies the Keeper, policy, Jira projects, and local readiness before it prepares any join request." />
            </ol>
          </div>
          {generatedHandoff?.kind === "connection" ? <ApiaryGeneratedLink link={generatedHandoff.link} onCopy={copyHandoffLink} /> : null}
          <div className="apiary-join-card">
            <div>
              <strong>Paste the Keeper's invitation link</strong>
              <small>Reviewing the link does not send anything or join the Apiary. Its private payload stays after the # fragment and is not sent during web navigation.</small>
            </div>
            <ApiaryLinkEntry label="Invitation link" value={invitationLink} action="Review invitation" disabled={busy || working} onChange={setInvitationLink} onAction={previewJoinInvitationLink} />
            <ApiaryFileFallback summary="Use an invitation file instead" ariaLabel="Choose Apiary invitation" disabled={busy || working} label="Choose invitation file" detail="or drop the Keeper's .json invitation here" onFile={(file) => void previewJoinInvitation(file)} />
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
                <strong>Invite a Hive</strong>
                <small>The other operator sends you a short-lived connection link. Swarm verifies its signed identity before it enables an invitation.</small>
              </div>
              <ol className="apiary-exchange-guide apiary-keeper-exchange" aria-label="How to invite a Hive">
                <ApiaryExchangeStep number="1" title="Receive her connection link" detail="Ask the Hive operator to copy her link under Join another Keeper's Apiary and send it privately to you." />
                <ApiaryExchangeStep number="2" title="Verify the exact identity" detail="Paste the link below. Swarm verifies its signature and shows the Hive and operator before any invitation exists." />
                <ApiaryExchangeStep number="3" title="Return the invitation link" detail="Create invitation copies one bounded link for that exact Hive. It expires and its secret can be consumed only once." />
              </ol>
              <ApiaryLinkEntry label="Hive connection link" value={connectionLink} action={working ? "Verifying…" : "Verify Hive"} disabled={busy || working} onChange={setConnectionLink} onAction={() => void importConnectionLink()} />
              <ApiaryFileFallback summary="Use a connection file instead" ariaLabel="Choose Hive connection card" disabled={busy || working} label={working ? "Verifying…" : "Choose connection card"} detail="or drop the Hive's .json connection card here" onFile={(file) => void importConnectionCard(file)} />
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
              {generatedHandoff?.kind === "invitation" ? <ApiaryGeneratedLink link={generatedHandoff.link} onCopy={copyHandoffLink} /> : null}
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

type ApiaryExchangeStepProps = {
  number: string;
  title: string;
  detail: string;
  children?: ReactNode;
};

function ApiaryExchangeStep({ number, title, detail, children }: ApiaryExchangeStepProps) {
  return (
    <li>
      <span className="apiary-step-number" aria-hidden="true">{number}</span>
      <span className="apiary-step-copy"><strong>{title}</strong><small>{detail}</small></span>
      {children ? <span className="apiary-step-action">{children}</span> : null}
    </li>
  );
}

function ApiaryGeneratedLink({ link, onCopy }: { link: string; onCopy: (link: string) => Promise<boolean> }) {
  const id = useId();
  return (
    <div className="apiary-generated-link" role="group" aria-label="Created Apiary link">
      <label htmlFor={id}><span>Private handoff link</span><input id={id} readOnly value={link} onFocus={(event) => event.currentTarget.select()} /></label>
      <button className="secondary-button" onClick={() => void onCopy(link)}>Copy again</button>
      <small>Share only with the intended operator. The signed payload expires; invitation secrets are bound to one Hive and consumed once.</small>
    </div>
  );
}

type ApiaryLinkEntryProps = {
  label: string;
  value: string;
  action: string;
  disabled: boolean;
  onChange: (value: string) => void;
  onAction: () => void;
};

function ApiaryLinkEntry({ label, value, action, disabled, onChange, onAction }: ApiaryLinkEntryProps) {
  const id = useId();
  return (
    <div className="apiary-link-entry">
      <label htmlFor={id}><span>{label}</span><input id={id} type="url" value={value} placeholder="Paste the complete link" onChange={(event) => onChange(event.target.value)} /></label>
      <button className="primary-action" disabled={disabled || !value.trim()} onClick={onAction}>{action}</button>
    </div>
  );
}

function ApiaryFileFallback(props: ApiaryFileDropProps & { summary: string }) {
  return <details className="apiary-file-fallback"><summary>{props.summary}</summary><ApiaryFileDrop {...props} /></details>;
}

type ApiaryFileDropProps = {
  ariaLabel: string;
  disabled: boolean;
  label: string;
  detail: string;
  onFile: (file: File | undefined) => void;
};

function ApiaryFileDrop({ ariaLabel, disabled, label, detail, onFile }: ApiaryFileDropProps) {
  const [dragging, setDragging] = useState(false);
  const detailId = useId();
  return (
    <label
      className={`apiary-card-drop${dragging ? " drag-active" : ""}`}
      onDragEnter={(event) => { event.preventDefault(); if (!disabled) setDragging(true); }}
      onDragOver={(event) => event.preventDefault()}
      onDragLeave={() => setDragging(false)}
      onDrop={(event) => {
        event.preventDefault();
        setDragging(false);
        if (!disabled) onFile(event.dataTransfer.files[0]);
      }}
    >
      <input
        aria-label={ariaLabel}
        aria-describedby={detailId}
        type="file"
        accept="application/json,.json"
        disabled={disabled}
        onChange={(event) => { onFile(event.target.files?.[0]); event.currentTarget.value = ""; }}
      />
      <span>{label}</span>
      <small id={detailId}>{detail}</small>
    </label>
  );
}

function FederationTransportStatus({ readiness }: { readiness: FederationTransportReadiness | undefined }) {
  const presentation = !readiness
    ? {
        title: "Checking this Hive's network address",
        detail: "Swarm is verifying whether another Hive can reach this installation.",
        tone: "waiting",
      }
    : readiness.reachability === "remote_https"
      ? {
          title: "Reachable Hive URL ready",
          detail: "Other Hives can contact this installation at the signed HTTPS address below. If this Hive becomes Keeper, it must remain online for invitations and shared coordination.",
          tone: "online",
        }
      : readiness.reachability === "local_only"
        ? {
            title: "Local testing only",
            detail: "A localhost or loopback address reaches only this machine. Configure a reachable HTTPS URL before another computer joins this Apiary.",
            tone: "waiting",
          }
        : {
            title: "Reachable Hive URL required",
            detail: "Configure this installation's public HTTPS URL before exchanging Apiary invitations. A private-network hostname is fine when every Hive can resolve and reach it.",
            tone: "offline",
          };
  return (
    <div className="apiary-network-readiness" aria-label="Hive network readiness" aria-live="polite">
      <span className={`presence ${presentation.tone}`} />
      <span>
        <strong>{presentation.title}</strong>
        <small>{presentation.detail}</small>
        {readiness?.endpoint ? <code>{readiness.endpoint}</code> : null}
      </span>
    </div>
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

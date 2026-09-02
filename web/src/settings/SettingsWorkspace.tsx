import { useEffect, useMemo, useRef, useState } from "react";

import { SETTINGS_CARDS, filterSettingsCards, type SettingsSection } from "./settingsNavigation";

import { downloadDatabaseBackup, draftWorkerDescription, fetchCoordinatorStatus, fetchEmailReadiness, fetchJiraReadiness, fetchQueenAutomationStatus, fetchTerminalHostStatus, fetchToolSurfaceStatus, improveWorkerDescription, runQueenAutomation, setQueenAutomationEnabled, type ControlRoomEvent, type CoordinatorStatus, type EmailReadiness, type Health, type HiveIdentity, type JiraReadiness, type NotificationPolicy, type NotificationSettings, type OperatorPresence, type PresenceMode, type ProviderCapabilities, type ProviderKind, type QueenAutomationStatus, type QueenAutonomyLevel, type QueenAutonomyPolicy, type SessionSummary, type TerminalHostStatus, type Worker, type WorkspaceChoice } from "../api";
import { downloadBlob } from "../shared/download";
import type { ColorTheme } from "../brand/theme";
import type { LiveFeedState } from "../controlRoom/ControlRoomLiveFeed";
import type { LockDetectionState } from "../presence/PresenceController";
import { deviceClass } from "../presence/PresenceController";
import type { NotificationCapabilityState } from "../notifications/NotificationController";
import { queenAutomationStateDetail, queenAutomationStateLabel, queenAutomationStateTone } from "../orchestration/queenAutomationPresentation";
import { queenAutonomyDetail, queenAutonomyLabel } from "../orchestration/queenAutonomyPresentation";
import { engineUpdateCost, workerEngineUpdateRequired, workersMidCommand } from "../runtime/workerEngine";
import ApiarySettings from "./ApiarySettings";
import DevelopmentReloadAction from "./DevelopmentReloadAction";
import ReleaseUpdateAction from "./ReleaseUpdateAction";
import OperatorAccessSettings from "./OperatorAccessSettings";
import RemoteAccessSettings from "./RemoteAccessSettings";
import type { TunnelStatus } from "../api";
import ProviderReleaseAction from "./ProviderReleaseAction";
import { useDevelopmentRuntime } from "./useDevelopmentRuntime";
import DiagnosticsWorkspace from "./DiagnosticsWorkspace";
import EmailSettings from "./EmailSettings";
import ConnectionsSettings from "./ConnectionsSettings";
import JiraSettings from "./JiraSettings";
import LegacyMigrationSettings from "./LegacyMigrationSettings";
import WorkerSettings from "./WorkerSettings";
import { navigateToSettingsSection, readSettingsSection, SETTINGS_SECTIONS } from "./settingsNavigation";
import { compactRuntimeVersion, runtimeVersionIdentity } from "./runtimeVersion";

type Props = {
  /** The section the rail has selected. Only its cards render. */
  section: SettingsSection;
  /** What the operator typed into the settings filter, if anything. */
  query?: string;
  busy: boolean;
  workerEngineProgress?: string;
  colorTheme: ColorTheme;
  feedbackRevision: number;
  health: Health | undefined;
  hiveIdentity: HiveIdentity | undefined;
  liveFeedState: LiveFeedState;
  operatorToken: string;
  /** The app owns tunnel state; the rail and this card must agree on it. */
  publicAddress?: TunnelStatus;
  onPublicAddressChange?: (status: TunnelStatus) => void;
  presence: OperatorPresence | undefined;
  providers: ProviderCapabilities;
  providerCapabilitiesUnavailable?: boolean;
  lockDetectionState: LockDetectionState;
  notificationSettings: NotificationSettings | undefined;
  queenPolicy: QueenAutonomyPolicy | undefined;
  pendingQueenDecisionCount?: number;
  notificationState: NotificationCapabilityState;
  recentEvents: ControlRoomEvent[];
  sessions: SessionSummary[];
  workers: Worker[];
  workspaces: WorkspaceChoice[];
  onThemeChange: (theme: ColorTheme) => void;
  onPresenceChange: (mode: PresenceMode | null) => Promise<void>;
  /** The screen Swarm opens on, one choice for every device. */
  startSurface: string;
  onStartSurfaceChange: (surface: string) => void;
  /** Requires the operator token again on this device. */
  onLock: () => void;
  onEnableLockDetection: () => Promise<void>;
  onNotificationPolicyChange: (policy: NotificationPolicy) => Promise<void>;
  onQueenPolicyChange: (policy: QueenAutonomyPolicy) => Promise<void>;
  onOpenQueenDecisions?: () => void;
  onOpenTasks?: () => void;
  onEnableNotifications: () => Promise<void>;
  onDisableNotifications: () => Promise<void>;
  onTestNotification: () => Promise<void>;
  onCreateWorker: (name: string, workspace: string, provider: ProviderKind, allowOutsideRoots: boolean) => Promise<void>;
  onUpdateWorker: (workerId: string, name: string, description: string, provider: ProviderKind, autostart: boolean, workspace?: string, allowOutsideRoots?: boolean) => Promise<void>;
  /** Applies a chosen bee on its own, without the rest of the worker form. */
  onChooseWorkerMark: (workerId: string, mark: string) => Promise<void>;
  onRemoveWorker: (workerId: string) => Promise<void>;
  onReorderWorkers: (workerIds: string[]) => Promise<void>;
  onRestartProviders: () => Promise<void>;
  onUpdateWorkerEngine: () => Promise<void>;
  onForceWorkerReload: () => Promise<void>;
  onReloadDevelopment: () => Promise<void>;
  onHiveIdentityChange: (identity: HiveIdentity) => void;
};

export default function SettingsWorkspace({ section, query = "", busy, workerEngineProgress, colorTheme, feedbackRevision, health, hiveIdentity, liveFeedState, operatorToken, publicAddress, onPublicAddressChange, presence, startSurface, onStartSurfaceChange, onLock, providers, providerCapabilitiesUnavailable = false, lockDetectionState, notificationSettings, queenPolicy, pendingQueenDecisionCount = 0, notificationState, recentEvents, sessions, workers, workspaces, onThemeChange, onPresenceChange, onEnableLockDetection, onNotificationPolicyChange, onQueenPolicyChange, onOpenQueenDecisions, onOpenTasks, onEnableNotifications, onDisableNotifications, onTestNotification, onCreateWorker, onUpdateWorker,
  onChooseWorkerMark, onRemoveWorker, onReorderWorkers, onRestartProviders, onUpdateWorkerEngine, onForceWorkerReload, onReloadDevelopment, onHiveIdentityChange }: Props) {
  const mobile = deviceClass() === "mobile";
  const [terminalHostStatus, setTerminalHostStatus] = useState<TerminalHostStatus>();
  const [terminalHostLoaded, setTerminalHostLoaded] = useState(false);
  const terminalHostLoadedRef = useRef(false);
  const [terminalHostAttempt, setTerminalHostAttempt] = useState(0);
  const [backupState, setBackupState] = useState<"idle" | "downloading" | "downloaded" | "error">("idle");
  const { runtime: developmentRuntime, reachable: developmentReachable } =
    useDevelopmentRuntime(operatorToken, health?.version);
  const [jiraReadiness, setJiraReadiness] = useState<JiraReadiness>();
  const [jiraUnavailable, setJiraUnavailable] = useState(false);
  const [jiraReadinessAttempt, setJiraReadinessAttempt] = useState(0);
  const [emailReadiness, setEmailReadiness] = useState<EmailReadiness>();
  const [emailUnavailable, setEmailUnavailable] = useState(false);
  const [emailReadinessAttempt, setEmailReadinessAttempt] = useState(0);
  const [queenAutomation, setQueenAutomation] = useState<QueenAutomationStatus>();
  const [queenAutomationBusy, setQueenAutomationBusy] = useState(false);
  const [queenAutomationError, setQueenAutomationError] = useState<string>();
  const [coordinatorStatus, setCoordinatorStatus] = useState<CoordinatorStatus>();
  const [confirmMaintenance, setConfirmMaintenance] = useState(false);
  const [confirmForceReload, setConfirmForceReload] = useState(false);
  /**
   * Whether live sessions can call what this build serves.
   *
   * Read here rather than derived from anything on the board: a session caches
   * its MCP tool list at connect and never asks again, so this is knowable only
   * from what the API has been asked for since it started.
   */
  const [toolSurface, setToolSurface] = useState<{ serving_revision: number; live_sessions: number; current: number; stale: number; unknown: number }>();
  useEffect(() => {
    if (!operatorToken) return;
    let current = true;
    const load = () => void fetchToolSurfaceStatus(operatorToken)
      .then((status) => { if (current) setToolSurface(status); })
      .catch(() => undefined);
    load();
    const timer = window.setInterval(load, 30_000);
    return () => { current = false; window.clearInterval(timer); };
  }, [operatorToken]);
  const surfaceUnreachable = (toolSurface?.stale ?? 0) + (toolSurface?.unknown ?? 0);
  const workspaceRef = useRef<HTMLDivElement>(null);
  // Choosing a section puts you at the top of it.
  //
  // This replaces two effects that chased a section down a long scrolling page
  // with a requestAnimationFrame and two timers each, re-running whenever the
  // layout crossed phone width. All of that existed because the page rendered
  // every card at once and a linked section had to be found among them. It
  // renders one section now, so there is nothing to find.
  useEffect(() => {
    workspaceRef.current?.scrollTo?.({ top: 0, behavior: "auto" });
  }, [section]);
  useEffect(() => {
    let cancelled = false;
    void fetchTerminalHostStatus(operatorToken)
      .then((status) => {
        if (!cancelled) {
          setTerminalHostStatus(status);
          terminalHostLoadedRef.current = true;
          setTerminalHostLoaded(true);
        }
      })
      .catch(() => {
        if (!cancelled && !terminalHostLoadedRef.current) {
          setTerminalHostStatus(undefined);
          terminalHostLoadedRef.current = true;
          setTerminalHostLoaded(true);
        }
      });
    return () => { cancelled = true; };
  }, [operatorToken, terminalHostAttempt, workerEngineProgress]);
  useEffect(() => {
    let cancelled = false;
    void fetchJiraReadiness(operatorToken)
      .then((readiness) => { if (!cancelled) { setJiraReadiness(readiness); setJiraUnavailable(false); } })
      .catch(() => { if (!cancelled) setJiraUnavailable(true); });
    return () => { cancelled = true; };
  }, [jiraReadinessAttempt, operatorToken]);
  const latestEventSequence = recentEvents.reduce((latest, event) => Math.max(latest, event.sequence), 0);
  useEffect(() => {
    let cancelled = false;
    void Promise.all([fetchQueenAutomationStatus(operatorToken), fetchCoordinatorStatus(operatorToken)])
      .then(([status, coordinator]) => {
        if (!cancelled) {
          setQueenAutomation(status);
          setCoordinatorStatus(coordinator);
          setQueenAutomationError(undefined);
        }
      })
      .catch(() => {
        if (!cancelled) setQueenAutomationError("Queen automation status is temporarily unavailable.");
      });
    return () => { cancelled = true; };
  }, [operatorToken, latestEventSequence]);
  useEffect(() => {
    let cancelled = false;
    void fetchEmailReadiness(operatorToken)
      .then((readiness) => { if (!cancelled) { setEmailReadiness(readiness); setEmailUnavailable(false); } })
      .catch(() => { if (!cancelled) setEmailUnavailable(true); });
    return () => { cancelled = true; };
  }, [emailReadinessAttempt, operatorToken]);
  async function downloadBackup() {
    setBackupState("downloading");
    try {
      const blob = await downloadDatabaseBackup(operatorToken);
      downloadBlob(blob, `swarm-next-hive-${new Date().toISOString().slice(0, 10)}.sqlite3`);
      setBackupState("downloaded");
    } catch {
      setBackupState("error");
    }
  }
  const workerEngineNeedsUpdate = workerEngineUpdateRequired(health, terminalHostStatus);
  const workerEngineState = !terminalHostLoaded ? "checking" : !terminalHostStatus ? "unavailable" : workerEngineNeedsUpdate ? "restart" : "current";
  const activeWorkerCount = terminalHostStatus?.running_sessions ?? 0;
  // Two different questions, answered by whichever source actually knows. The
  // host knows how many sessions it will stop; the roster knows which of them
  // are mid-command, which is the part that costs the operator something.
  const busyWorkerNames = workersMidCommand(workers);
  const hasPendingQueenDecision = pendingQueenDecisionCount > 0;
  const queenReviewLabel = hasPendingQueenDecision ? "Queen needs you" : queenAutomationStateLabel(queenAutomation);
  const queenReviewDetail = hasPendingQueenDecision
    ? `${pendingQueenDecisionCount} specific Queen decision${pendingQueenDecisionCount === 1 ? " is" : "s are"} waiting in Needs you. Resolve ${pendingQueenDecisionCount === 1 ? "it" : "them"} there; Swarm will not repeat the review.`
    : queenAutomationStateDetail(queenAutomation);
  const queenReviewTone = hasPendingQueenDecision ? "offline" : queenAutomationStateTone(queenAutomation);
  // One section at a time, or the filter's results across all of them.
  //
  // The page used to render all fourteen cards at once behind a rail that
  // listed ten, so four of them existed only for whoever scrolled past the
  // right spot. Selecting a section is now the whole answer to "where is it",
  // and the filter is the answer when the operator does not know which section
  // to look in.
  const matched = useMemo(
    () => new Set(filterSettingsCards(query).map((card) => card.id)),
    [query],
  );
  const filtering = query.trim().length > 0;
  const shows = (id: string) =>
    filtering ? matched.has(id) : SETTINGS_CARDS.some((card) => card.id === id && card.section === section);

  return (
    <div className="settings-workspace" ref={workspaceRef}>
      {filtering ? (
        <p className="settings-filter-summary" role="status">
          {matched.size === 0
            ? `Nothing in Settings matches "${query.trim()}".`
            : `${matched.size} ${matched.size === 1 ? "setting matches" : "settings match"} "${query.trim()}".`}
        </p>
      ) : null}
      {/* The section list lives in the rail now, with every other surface's
          navigation. */}
      {shows("settings-crew") && (
    <WorkerSettings workers={workers} workspaces={workspaces} busy={busy} providers={providers} providerCapabilitiesUnavailable={providerCapabilitiesUnavailable} onCreate={onCreateWorker} onUpdate={onUpdateWorker} onChooseMark={onChooseWorkerMark} onRemove={onRemoveWorker} onDraftDescription={async (workerId) => (await draftWorkerDescription(operatorToken, workerId)).description} onImproveDescription={async (workerId) => (await improveWorkerDescription(operatorToken, workerId)).description} onReorder={onReorderWorkers} />
      )}
      {shows("settings-presence") && (
    <section id="settings-presence" className="settings-card presence-settings" aria-labelledby="presence-heading">
          <div><p className="eyebrow">Presence</p><h3 id="presence-heading">Let attention follow you</h3></div>
          {/* One choice for every device. A phone opening somewhere a desktop
              would not is the thing this exists to stop, so it is deliberately
              not a per-device preference like the theme below it. */}
          <div className="settings-lock">
            <div><strong>Lock Swarm</strong><small>Asks for the operator token again on this device. Locking your computer does not require this.</small></div>
            <button type="button" className="secondary-button" disabled={busy} onClick={onLock}>Lock</button>
          </div>
          <label htmlFor="start-surface"><span>Opening screen</span>
            <select
              id="start-surface"
              value={startSurface}
              disabled={busy}
              onChange={(event) => onStartSurfaceChange(event.target.value)}
            >
              <option value="decisions">Needs you</option>
              <option value="tasks">Task board</option>
              <option value="workers">Workers</option>
              <option value="apiary">Apiary</option>
              <option value="settings">Settings</option>
            </select>
            <small>Every device opens here.</small>
          </label>
          <p>Automatic presence uses this device's activity, visibility, and expiry. A manual mode stays in effect until you return to Automatic.</p>
          <label htmlFor="presence-mode"><span>Presence policy</span>
            <select
              id="presence-mode"
              value={presence?.manual_mode ?? "auto"}
              onChange={(event) => void onPresenceChange(event.target.value === "auto" ? null : event.target.value as PresenceMode)}
            >
              <option value="auto">Automatic</option>
              <option value="at_hive">At the Hive</option>
              <option value="away">Away</option>
              <option value="night_watch">Night Watch</option>
            </select>
          </label>
          <div className="presence-summary" role="status">
            <span className={`presence ${presence?.mode === "at_hive" ? "online" : presence?.mode === "night_watch" ? "waiting" : "offline"}`} />
            <span><strong>{presenceLabel(presence?.mode)}</strong><small>{presenceSourceLabel(presence)}</small></span>
          </div>
          {!mobile && <div className="presence-lock-row">
            <div><strong>Computer lock detection</strong><small>{lockDetectionLabel(lockDetectionState)}</small></div>
            <button className="secondary-button" disabled={lockDetectionState === "unsupported" || lockDetectionState === "enabling" || lockDetectionState === "enabled"} onClick={() => void onEnableLockDetection()}>
              {lockDetectionState === "enabled" ? "Enabled" : lockDetectionState === "enabling" ? "Enabling…" : "Enable"}
            </button>
          </div>}
          {mobile && <p className="mobile-presence-note">Your workstation reports when it is locked. This phone follows that presence and carries notifications when you are away.</p>}
        </section>
      )}
      {shows("settings-queen") && (
    <section id="settings-queen" className="settings-card queen-policy-settings" aria-labelledby="queen-policy-heading">
          <div><p className="eyebrow">Queen autonomy</p><h3 id="queen-policy-heading">Choose how far she may carry work</h3></div>
          <p>Presence changes the ceiling for unattended work. Night Watch can use durable rules you already approved so the Hive keeps moving while you sleep. These limits never expand from model confidence.</p>
          <div className="queen-policy-grid">
            {(["at_hive", "away", "night_watch"] as const).map((mode) => (
              <label key={mode} htmlFor={`queen-policy-${mode}`}><span>{presenceLabel(mode)}</span>
                <select
                  id={`queen-policy-${mode}`}
                  value={queenPolicy?.[mode] ?? "coordinate"}
                  disabled={busy || !queenPolicy}
                  onChange={(event) => void onQueenPolicyChange({ ...queenPolicy!, [mode]: event.target.value as QueenAutonomyLevel })}
                >
                  <option value="advisory">Advise only</option>
                  <option value="coordinate">Coordinate the Hive</option>
                  <option value="local_execution">Run approved work</option>
                </select>
              </label>
            ))}
          </div>
          <div className="queen-policy-explainer" aria-label="Queen autonomy levels">
            {(["advisory", "coordinate", "local_execution"] as const).map((level) => (
              <div key={level}><strong>{queenAutonomyLabel(level)}</strong><small>{queenAutonomyDetail(level)}</small></div>
            ))}
          </div>
          <div className="queen-conductor" aria-labelledby="queen-conductor-heading">
            <div className="queen-conductor-heading">
              <div>
                <strong id="queen-conductor-heading">Automatic work review</strong>
                <small>Queen notices durable task changes and coordinates local workers within the presence policy.</small>
              </div>
              <label className="queen-automation-toggle">
                <input
                  type="checkbox"
                  checked={queenAutomation?.enabled ?? false}
                  disabled={busy || queenAutomationBusy || !queenAutomation}
                  onChange={async (event) => {
                    setQueenAutomationBusy(true);
                    setQueenAutomationError(undefined);
                    try {
                      setQueenAutomation(await setQueenAutomationEnabled(operatorToken, event.target.checked));
                    } catch {
                      setQueenAutomationError("Queen automation could not be changed. Try again when the connection is stable.");
                    } finally {
                      setQueenAutomationBusy(false);
                    }
                  }}
                />
                <span>{queenAutomation?.enabled ? "Automatic on" : "Automatic off"}</span>
              </label>
            </div>
            <div className={`queen-automation-status ${queenAutomation?.state ?? "idle"}`} role="status">
              <span className={`presence ${queenReviewTone}`} />
              <span>
                <strong>{queenReviewLabel}</strong>
                <small>{queenReviewDetail}</small>
              </span>
            </div>
            <div className={`coordinator-status ${coordinatorStatus?.uncertain_actions ? "needs-attention" : ""}`} aria-label="Deterministic coordinator status">
              <span>
                <strong>Routine coordination</strong>
                <small>Swarm loads assigned sleeping workers, then surfaces delivered work that never starts, becomes stale, or loses its worker.</small>
                <small>{coordinatorAdmissionDetail(coordinatorStatus?.automatic_start_admission, coordinatorStatus?.automatic_start_batch_limit)}</small>
              </span>
              <span className="coordinator-metrics">
                <strong>{coordinatorStatus?.queen_calls_avoided ?? 0}</strong>
                <small>Queen reviews avoided</small>
              </span>
              <span className="coordinator-metrics">
                <strong>{coordinatorStatus?.queued_actions ?? 0}</strong>
                <small>Worker starts queued</small>
              </span>
              <span className="coordinator-metrics">
                <strong>{coordinationAttentionTotal(coordinatorStatus)}</strong>
                <small>{coordinationAttentionDetail(coordinatorStatus)}</small>
              </span>
              <span className="coordinator-metrics">
                <strong>{coordinatorStatus?.uncertain_actions ?? 0}</strong>
                <small>Worker cases needing judgment</small>
              </span>
            </div>
            {queenAutomationError && <p className="queen-automation-error" role="alert">{queenAutomationError}</p>}
            <div className="queen-conductor-actions">
              <button
                type="button"
                className="secondary-button"
                disabled={busy || queenAutomationBusy || (!hasPendingQueenDecision && (!queenAutomation || ["queued", "delivering", "running"].includes(queenAutomation.state)))}
                onClick={async () => {
                  if (hasPendingQueenDecision) {
                    onOpenQueenDecisions?.();
                    return;
                  }
                  setQueenAutomationBusy(true);
                  setQueenAutomationError(undefined);
                  try {
                    setQueenAutomation(await runQueenAutomation(operatorToken));
                  } catch {
                    setQueenAutomationError("Queen could not start a review. Her current work was not changed.");
                  } finally {
                    setQueenAutomationBusy(false);
                  }
                }}
              >{hasPendingQueenDecision ? `Review ${pendingQueenDecisionCount === 1 ? "decision" : "decisions"}` : queenAutomationBusy ? "Checking…" : queenAutomation?.state === "uncertain" ? "Retry Queen review" : "Run Queen now"}</button>
              <small>{hasPendingQueenDecision ? "The existing request is the authoritative next step." : "Manual review works even when automatic review is off."}</small>
            </div>
            <p className="queen-conductor-boundary">She pauses while you are working with her. External effects remain blocked unless an exact action is covered by a durable operator-approved rule; Queen cannot create or widen that authority.</p>
          </div>
          <small className="privacy-note">A deployment rule grants only its recorded repository, environment, action, and limits. Anything outside that scope returns to Needs you. Scout and repository workers still perform implementation.</small>
        </section>
      )}
      {shows("settings-notifications") && (
    <section id="settings-notifications" className="settings-card notification-settings" aria-labelledby="notification-heading">
          <div><p className="eyebrow">Mobile attention</p><h3 id="notification-heading">Let urgent work find you</h3></div>
          <p>Notifications are quiet while you are At the Hive. Away and Night Watch can deliver generic, private prompts when a worker needs a decision.</p>
          {!mobile ? <label htmlFor="notification-policy"><span>Notify me</span>
            <select
              id="notification-policy"
              value={notificationSettings?.policy ?? "important_only"}
              disabled={!notificationSettings}
              onChange={(event) => void onNotificationPolicyChange(event.target.value as NotificationPolicy)}
            >
              <option value="important_only">Important or time-sensitive decisions</option>
              <option value="all_decisions">Every pending decision</option>
              <option value="off">Never</option>
            </select>
          </label> : (
            <fieldset className="mobile-policy-choice">
              <legend>Notify me</legend>
              {([
                ["important_only", "Important only", "Urgent or time-sensitive decisions"],
                ["all_decisions", "Every decision", "All new items that need you"],
                ["off", "Off", "Keep notifications on this device quiet"],
              ] as const).map(([value, label, detail]) => (
                <label key={value}>
                  <span><strong>{label}</strong><small>{detail}</small></span>
                  <input
                    type="radio"
                    name="notification-policy-mobile"
                    value={value}
                    checked={(notificationSettings?.policy ?? "important_only") === value}
                    disabled={!notificationSettings}
                    onChange={() => void onNotificationPolicyChange(value)}
                  />
                </label>
              ))}
            </fieldset>
          )}
          <div className="notification-status" role="status">
            <span className={`presence ${notificationState === "enabled" ? "online" : notificationState === "error" || notificationState === "denied" ? "offline" : "waiting"}`} />
            <span><strong>{notificationStateLabel(notificationState)}</strong><small>{notificationStateDetail(notificationState, notificationSettings?.subscription_count ?? 0)}</small></span>
          </div>
          <div className="settings-actions">
            {notificationState === "enabled" ? (
              <button className="secondary-button" onClick={() => void onDisableNotifications()}>Disable this device</button>
            ) : (
              <button className="primary-action" disabled={notificationState === "unsupported" || notificationState === "denied" || notificationState === "enabling" || !notificationSettings} onClick={() => void onEnableNotifications()}>{notificationState === "enabling" ? "Enabling…" : notificationState === "error" ? "Repair this device" : "Enable this device"}</button>
            )}
            <button className="secondary-button" disabled={notificationState !== "enabled"} onClick={() => void onTestNotification()}>Test this device</button>
          </div>
          <small className="privacy-note">Notification text never includes repository names, task titles, evidence, credentials, or terminal output.</small>
        </section>
      )}
      {shows("settings-appearance") && (
    <section id="settings-appearance" className="settings-card" aria-labelledby="appearance-heading">
          <div><p className="eyebrow">Appearance</p><h3 id="appearance-heading">Comfortable in long sessions</h3></div>
          <p>Both themes use the same soft natural palette and high-legibility type system. This choice follows your {mobile ? "mobile" : "desktop"} profile and is included in Hive backups.</p>
          <div className="theme-choice" role="group" aria-label="Color theme">
            <button aria-pressed={colorTheme === "light"} onClick={() => onThemeChange("light")}><span className="theme-swatch light" /> Light meadow</button>
            <button aria-pressed={colorTheme === "dark"} onClick={() => onThemeChange("dark")}><span className="theme-swatch dark" /> Night hive</button>
          </div>
        </section>
      )}

      <section className="settings-card" aria-labelledby="identity-heading">
        <div><p className="eyebrow">Identity</p><h3 id="identity-heading">Your Hive</h3></div>
        <p>This local boundary owns its workers, tasks, repositories, and provider sessions.</p>
        <dl className="diagnostic-list">
          <div><dt>Hive</dt><dd>{hiveIdentity?.hive.name ?? "Unavailable"}</dd></div>
          <div><dt>Operator</dt><dd>{hiveIdentity?.operator.display_name ?? "Unavailable"}</dd></div>
          <div><dt>Membership</dt><dd>{apiaryMembershipLabel(hiveIdentity)}</dd></div>
        </dl>
      </section>

      {shows("settings-apiary") && (
    <ApiarySettings busy={busy} hiveIdentity={hiveIdentity} operatorToken={operatorToken} onHiveIdentityChange={onHiveIdentityChange} />
      )}

      {shows("settings-runtime") && (
    <section id="settings-runtime" className="settings-card" aria-labelledby="runtime-heading">
          <div><p className="eyebrow">Runtime</p><h3 id="runtime-heading">Local system</h3></div>
          <dl className="diagnostic-list runtime-summary-list">
            <div><dt>App/API</dt><dd>{health ? compactRuntimeVersion(health.version) : "Unavailable"}</dd></div>
            <div><dt>Live updates</dt><dd>{liveFeedLabel(liveFeedState)}</dd></div>
            <div><dt>Running workers</dt><dd>{workers.filter((worker) => worker.running).length}</dd></div>
            <div><dt>Retained sessions</dt><dd>{sessions.length}</dd></div>
          </dl>
          <div className="runtime-subsystem-grid">
            <article className={`runtime-subsystem-card runtime-subsystem-${workerEngineState}`} aria-label="Worker engine status">
              <header><div><span className="runtime-component-name">Worker engine</span><strong>{workerEngineLabel(health, terminalHostStatus, terminalHostLoaded)}</strong></div><span className={`runtime-status-badge ${workerEngineState}`}>{workerEngineState === "restart" ? "Restart required" : workerEngineState === "unavailable" ? "Unavailable" : workerEngineState === "checking" ? "Checking" : "Current"}</span></header>
              {/* RUNNING, NOT INSTALLED, and the difference is not pedantry.
                  host_version is what the host PROCESS reports, and it can
                  differ from what host-current points at: a reload moves the
                  symlink, and the reconcile correctly declines to restart when
                  the engine is unchanged. Measured on 2026-09-01 the host was
                  executing 0a2f75c while host-current read d3bd9d9 — and the
                  card called the first one "Installed", which is the one word
                  it certainly was not. Anybody checking which build the engine
                  is on had to read /proc to find out. */}
              <p className="runtime-version"><strong>Running</strong> {runtimeVersionIdentity(terminalHostStatus?.host_version)}</p>
              {health?.version && terminalHostStatus?.host_version && health.version !== terminalHostStatus.host_version ? (
                <p className="runtime-version"><strong>Installed</strong> {runtimeVersionIdentity(health.version)}</p>
              ) : null}
              {workerEngineState === "checking" ? <p>Checking the separate worker engine without interrupting any terminal.</p> : workerEngineState === "unavailable" ? <>
                <p>Swarm could not confirm the worker engine’s health or installed version. Existing worker processes may still be running.</p>
                <small>No restart or update has been attempted.</small>
                <button className="secondary-button" type="button" onClick={() => setTerminalHostAttempt((attempt) => attempt + 1)}>Retry worker engine status</button>
              </> : workerEngineNeedsUpdate ? <>
                <p>Updating this layer briefly stops {activeWorkerCount} active worker{activeWorkerCount === 1 ? "" : "s"}, then brings back the ones that were loaded from their saved conversations.</p>
                <p className={busyWorkerNames.length > 0 ? "engine-update-cost busy" : "engine-update-cost"} role="status">{engineUpdateCost(busyWorkerNames)}</p>
                <small>Identities, provider conversations, tasks, ownership, and terminal history remain durable.</small>
              {workerEngineProgress ? (
                <div className="maintenance-progress" role="status" aria-live="polite">
                  <span className="maintenance-spinner" aria-hidden="true" />
                  <div><strong>Updating worker engine…</strong><span>{workerEngineProgress}</span></div>
                </div>
              ) : !confirmMaintenance ? (
                <button className="secondary-button" disabled={busy} onClick={() => setConfirmMaintenance(true)}>Prepare worker engine update</button>
              ) : (
                <div className="maintenance-confirmation" role="group" aria-label="Confirm worker engine update">
                  <strong>Restart {activeWorkerCount} active worker{activeWorkerCount === 1 ? "" : "s"} now?</strong>
                  <span>{engineUpdateCost(busyWorkerNames)}</span>
                  <span>Claude/Codex processes will close. Worker identities, tasks, and known conversation IDs remain durable.</span>
                  <div className="settings-actions">
                    <button className="secondary-button" disabled={busy} onClick={() => setConfirmMaintenance(false)}>Not now</button>
                    <button className="primary-action" disabled={busy} onClick={() => { setConfirmMaintenance(false); void onUpdateWorkerEngine(); }}>Stop workers and update</button>
                  </div>
                </div>
              )}</> : <><p>The installed worker engine is compatible with this App/API release. Running workers do not need to restart.</p><small>Claude and Codex processes remain attached to this engine across ordinary app and API reloads.</small></>}
              {/* ALWAYS AVAILABLE, INCLUDING WHEN THE ENGINE IS CURRENT — that
                  is the whole reason it exists. A worker caches its MCP tool
                  list when it connects, so a change to the agent tool surface
                  reaches nobody until the session reconnects, and nothing
                  announces it: the card above correctly says the engine is
                  current, because it is. On 2026-09-02 the API served tool
                  surface revision 11 while all 13 live sessions still held
                  revision 10, and the Hive looked entirely healthy.

                  Deliberately NOT automatic. Ending every session is the act
                  the operator has held repeatedly; a button is how they choose
                  the moment. */}
              <div className="force-worker-reload">
                {/* STALE AND UNKNOWN ARE BOTH REPORTED, and unknown is not
                    folded into healthy. A session that has not asked THIS build
                    for its tools is not known to be fine — the record is in
                    memory and an API restart empties it while the sessions
                    survive. Saying "13 could not be confirmed" is the whole
                    point; saying nothing is what happened before. */}
                {surfaceUnreachable > 0 ? (
                  <p className="tool-surface-warning" role="status">
                    {toolSurface?.stale ? `${toolSurface.stale} session${toolSurface.stale === 1 ? "" : "s"} hold an older tool list` : null}
                    {toolSurface?.stale && toolSurface?.unknown ? ", and " : null}
                    {toolSurface?.unknown ? `${toolSurface.unknown} could not be confirmed` : null}
                    {" "}against revision {toolSurface?.serving_revision}. They cannot use a tool
                    added since they connected. Only a session restart fixes this — a reload does
                    not, and it is not a worker engine update.
                  </p>
                ) : null}
                {confirmForceReload ? (
                  <div className="maintenance-confirmation" role="group" aria-label="Confirm forced worker reload">
                    <strong>Restart {activeWorkerCount} worker{activeWorkerCount === 1 ? "" : "s"} now?</strong>
                    <span>Every live session ends and reconnects, which is what picks up a changed tool surface. Loaded workers come back from their saved conversations; identities, tasks and history are durable.</span>
                    <div className="settings-actions">
                      <button className="secondary-button" disabled={busy} onClick={() => setConfirmForceReload(false)}>Not now</button>
                      <button className="primary-action" disabled={busy} onClick={() => { setConfirmForceReload(false); void onForceWorkerReload(); }}>Restart every worker</button>
                    </div>
                  </div>
                ) : (
                  <>
                    {/* NOT disabled on activeWorkerCount === 0. That count is
                        terminalHostStatus.running_sessions, which is UNDEFINED
                        until the host answers and falls to 0 — so disabling on
                        it treats "I cannot tell yet" as "there is nothing to
                        restart", which is the same shape as every other
                        cannot-check-read-as-an-answer defect this Hive has
                        chased. The confirmation states the count instead. */}
                    <button className="secondary-button" disabled={busy} onClick={() => setConfirmForceReload(true)}>Force worker reload</button>
                    <small>Reconnects every worker so a changed agent tool surface reaches them. Needed after an API update that adds or changes a tool, which the engine status above cannot show.</small>
                  </>
                )}
              </div>
            </article>
            <DevelopmentReloadAction busy={busy} runtime={developmentRuntime} reachable={developmentReachable} healthVersion={health?.version} onReload={onReloadDevelopment} />
            <ReleaseUpdateAction busy={busy} operatorToken={operatorToken} />
            <ProviderReleaseAction superseded={providers?.superseded ?? []} busy={busy} onRestart={onRestartProviders} />
          </div>
        </section>
      )}

      {shows("settings-connections-tools") && (
    <ConnectionsSettings operatorToken={operatorToken} />
      )}
      {shows("settings-integrations") && (
    <JiraSettings operatorToken={operatorToken} readiness={jiraReadiness} unavailable={jiraUnavailable} onRetryReadiness={() => setJiraReadinessAttempt((attempt) => attempt + 1)} />
      )}
      {shows("settings-email") && (
    <EmailSettings operatorToken={operatorToken} readiness={emailReadiness} unavailable={emailUnavailable} onRetryReadiness={() => setEmailReadinessAttempt((attempt) => attempt + 1)} />
      )}

      {shows("settings-migration") && (
    <LegacyMigrationSettings busy={busy} operatorToken={operatorToken} onOpenTasks={onOpenTasks} />
      )}

      {shows("settings-access") && (
    <OperatorAccessSettings busy={busy} operatorToken={operatorToken} />
      )}
      {shows("settings-remote") && (
    <RemoteAccessSettings busy={busy} operatorToken={operatorToken} status={publicAddress} onStatusChange={onPublicAddressChange} />
      )}

      {shows("settings-backup") && (
    <section id="settings-backup" className="settings-card" aria-labelledby="backup-heading">
          <div><p className="eyebrow">Backup</p><h3 id="backup-heading">Carry your Hive safely</h3></div>
          <p>Download a consistent snapshot of workers, tasks, conversations, policies, and Hive identity. Repository contents are intentionally excluded.</p>
          <div className="settings-actions">
            <button className="primary-action" disabled={busy || backupState === "downloading"} onClick={() => void downloadBackup()}>{backupState === "downloading" ? "Preparing backup…" : backupState === "error" ? "Try backup again" : "Download Hive backup"}</button>
          </div>
          {backupState === "downloaded" ? <p className="form-message" role="status">Hive backup downloaded. Keep it somewhere private and durable.</p> : null}
          {backupState === "error" ? <p className="form-error" role="alert">The Hive backup could not be prepared. No local data was changed.</p> : null}
          <details className="restore-guide">
            <summary>How to restore this backup</summary>
            <ol>
              <li>Move the downloaded file to the Swarm host under your home folder.</li>
              <li>Run <code>swarm-next-package restore /home/you/path/to/swarm-next-backup.sqlite3</code>.</li>
              <li>Reopen Swarm. The API restarts, but running worker terminals and repositories stay in place.</li>
            </ol>
            <p>Restore verifies the backup first and creates a rollback snapshot before changing the Hive database.</p>
          </details>
          <small className="privacy-note">This file contains private operational data. Store it like a credential. Host credentials and repository contents remain intentionally separate.</small>
        </section>
      )}

      {shows("settings-diagnostics") && (
        <DiagnosticsWorkspace feedbackRevision={feedbackRevision} operatorToken={operatorToken} health={health} hiveIdentity={hiveIdentity} liveFeedState={liveFeedState} recentEvents={recentEvents} sessions={sessions} workers={workers} jiraReadiness={jiraReadiness} jiraUnavailable={jiraUnavailable} />
      )}

      {shows("settings-shortcuts") && (
      <section id="settings-shortcuts" className="settings-card shortcuts-card" aria-labelledby="shortcuts-heading">
        <div><p className="eyebrow">Keyboard</p><h3 id="shortcuts-heading">Move without losing focus</h3></div>
        <dl className="shortcut-list">
          <div><dt>Needs you</dt><dd><kbd>Alt</kbd><kbd>1</kbd></dd></div>
          <div><dt>Tasks</dt><dd><kbd>Alt</kbd><kbd>2</kbd></dd></div>
          <div><dt>Workers</dt><dd><kbd>Alt</kbd><kbd>3</kbd></dd></div>
          <div><dt>Settings</dt><dd><kbd>Alt</kbd><kbd>4</kbd></dd></div>
          <div><dt>Previous / next worker</dt><dd><kbd>Alt</kbd><kbd>↑</kbd> / <kbd>Alt</kbd><kbd>↓</kbd></dd></div>
          <div><dt>Quick navigation</dt><dd><kbd>Alt</kbd><kbd>K</kbd></dd></div>
        </dl>
        <p>Shortcuts pause while you type in a terminal, field, or menu.</p>
      </section>
      )}
    </div>
  );
}

function coordinatorAdmissionDetail(admission: CoordinatorStatus["automatic_start_admission"] | undefined, batchLimit: number | undefined) {
  const serialized = batchLimit === 1 ? " Swarm starts one worker at a time, then checks memory again." : "";
  switch (admission) {
    case "allowed": return `Automatic worker starts are available.${serialized}`;
    case "deferred_advisory": return `Automatic worker starts are waiting for memory pressure to settle.${serialized}`;
    case "deferred_critical": return `Automatic worker starts are paused to protect active work from critical memory pressure.${serialized}`;
    case "deferred_unavailable": return `Automatic worker starts are waiting for reliable worker-engine evidence.${serialized}`;
    default: return "Checking whether automatic worker starts are safe.";
  }
}

function coordinationAttentionTotal(status: CoordinatorStatus | undefined) {
  return (status?.unstarted_attention_actions ?? 0)
    + (status?.stale_attention_actions ?? 0)
    + (status?.worker_exit_attention_actions ?? 0);
}

function coordinationAttentionDetail(status: CoordinatorStatus | undefined) {
  if (!status || coordinationAttentionTotal(status) === 0) return "Worker cases surfaced";
  return `${status.unstarted_attention_actions} not started · ${status.stale_attention_actions} stale · ${status.worker_exit_attention_actions} exited`;
}

function workerEngineLabel(health: Health | undefined, host: TerminalHostStatus | undefined, loaded: boolean) {
  if (!loaded) return "Checking…";
  if (!host) return "Unavailable";
  if (workerEngineUpdateRequired(health, host)) {
    return "Update ready · restart required";
  }
  return `Current · ${host.running_sessions} active`;
}

function notificationStateLabel(state: NotificationCapabilityState) {
  if (state === "enabled") return "Ready on this device";
  if (state === "enabling") return "Enabling this device";
  if (state === "denied") return "Browser permission blocked";
  if (state === "unsupported") return "Push unavailable in this browser";
  if (state === "error") return "Connection needs repair";
  return "Available when you choose";
}

function notificationStateDetail(state: NotificationCapabilityState, count: number) {
  if (state === "enabled") return `${count} device${count === 1 ? "" : "s"} registered with this Hive`;
  if (state === "enabling") return "Waiting for the browser and Hive to confirm the subscription.";
  if (state === "denied") return "Allow notifications in browser site settings to enable them.";
  if (state === "unsupported") return "Presence and the Needs you inbox continue to work normally.";
  if (state === "error") return "Your browser permission is unchanged. Swarm will retry at startup, or you can repair this device now.";
  return "Nothing is registered until you explicitly enable it.";
}
function presenceLabel(mode: PresenceMode | undefined) {
  if (mode === "at_hive") return "At the Hive";
  if (mode === "night_watch") return "Night Watch";
  return mode === "away" ? "Away" : "Connecting…";
}

function presenceSourceLabel(presence: OperatorPresence | undefined) {
  if (!presence) return "Waiting for the first device observation";
  if (presence.source === "manual") return "Manual override";
  if (presence.source === "active_device") return "Active device detected";
  if (presence.source === "screen_locked") return "Computer lock detected";
  if (presence.source === "inactive_device") return "No active visible device";
  return "Device heartbeat expired safely";
}

function lockDetectionLabel(state: LockDetectionState) {
  if (state === "enabled") return "Locking this computer moves Automatic presence to Away.";
  if (state === "enabling") return "Waiting for the browser to finish enabling lock detection.";
  if (state === "available") return "Supported; enabling requires one browser permission.";
  if (state === "denied") return "Not granted; activity and visibility fallback remain active.";
  if (state === "error") return "Could not start; activity and visibility fallback remain active.";
  return "Unavailable in this browser; fallback presence remains active.";
}

function apiaryMembershipLabel(identity: HiveIdentity | undefined) {
  if (!identity) return "Unavailable";
  if (!identity.apiary_context) return identity.hive.apiary_id ? "Apiary member" : "Personal Hive";
  if (identity.apiary_context.mode === "personal") return "Personal Hive";
  const backend = identity.apiary_context.apiary.shared_work_backend === "jira" ? "Jira-backed" : "Native";
  const role = identity.apiary_context.local_role === "keeper" ? "Keeper" : "Member";
  return `${identity.apiary_context.apiary.name} · ${role} · ${backend}`;
}
function liveFeedLabel(state: LiveFeedState) {
  if (state === "connected") return "Connected";
  if (state === "retrying") return "Reconnecting";
  return "Connecting";
}

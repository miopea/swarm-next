import { useEffect, useState } from "react";

import { downloadDatabaseBackup, fetchEmailReadiness, fetchJiraReadiness, fetchTerminalHostStatus, type ControlRoomEvent, type EmailReadiness, type Health, type HiveIdentity, type JiraReadiness, type NotificationPolicy, type NotificationSettings, type OperatorPresence, type PresenceMode, type ProviderCapabilities, type ProviderKind, type QueenAutonomyLevel, type QueenAutonomyPolicy, type SessionSummary, type TerminalHostStatus, type Worker, type WorkspaceChoice } from "../api";
import { downloadBlob } from "../shared/download";
import type { ColorTheme } from "../brand/theme";
import type { LiveFeedState } from "../controlRoom/ControlRoomLiveFeed";
import type { LockDetectionState } from "../presence/PresenceController";
import { deviceClass } from "../presence/PresenceController";
import type { NotificationCapabilityState } from "../notifications/NotificationController";
import { workerEngineUpdateRequired } from "../runtime/workerEngine";
import ApiarySettings from "./ApiarySettings";
import DevelopmentReloadAction from "./DevelopmentReloadAction";
import { useDevelopmentRuntime } from "./useDevelopmentRuntime";
import DiagnosticsWorkspace from "./DiagnosticsWorkspace";
import EmailSettings from "./EmailSettings";
import JiraSettings from "./JiraSettings";
import WorkerSettings from "./WorkerSettings";
import { navigateToSettingsSection, readSettingsSection, SETTINGS_SECTIONS } from "./settingsNavigation";

type Props = {
  busy: boolean;
  colorTheme: ColorTheme;
  feedbackRevision: number;
  health: Health | undefined;
  hiveIdentity: HiveIdentity | undefined;
  liveFeedState: LiveFeedState;
  operatorToken: string;
  presence: OperatorPresence | undefined;
  providers: ProviderCapabilities;
  lockDetectionState: LockDetectionState;
  notificationSettings: NotificationSettings | undefined;
  queenPolicy: QueenAutonomyPolicy | undefined;
  notificationState: NotificationCapabilityState;
  recentEvents: ControlRoomEvent[];
  sessions: SessionSummary[];
  workers: Worker[];
  workspaces: WorkspaceChoice[];
  onThemeChange: (theme: ColorTheme) => void;
  onPresenceChange: (mode: PresenceMode | null) => Promise<void>;
  onEnableLockDetection: () => Promise<void>;
  onNotificationPolicyChange: (policy: NotificationPolicy) => Promise<void>;
  onQueenPolicyChange: (policy: QueenAutonomyPolicy) => Promise<void>;
  onEnableNotifications: () => Promise<void>;
  onDisableNotifications: () => Promise<void>;
  onTestNotification: () => Promise<void>;
  onCreateWorker: (name: string, workspace: string, provider: ProviderKind, allowOutsideRoots: boolean) => Promise<void>;
  onUpdateWorker: (workerId: string, name: string, autostart: boolean) => Promise<void>;
  onReorderWorkers: (workerIds: string[]) => Promise<void>;
  onUpdateWorkerEngine: () => Promise<void>;
  onReloadDevelopment: () => Promise<void>;
  onHiveIdentityChange: (identity: HiveIdentity) => void;
};

export default function SettingsWorkspace({ busy, colorTheme, feedbackRevision, health, hiveIdentity, liveFeedState, operatorToken, presence, providers, lockDetectionState, notificationSettings, queenPolicy, notificationState, recentEvents, sessions, workers, workspaces, onThemeChange, onPresenceChange, onEnableLockDetection, onNotificationPolicyChange, onQueenPolicyChange, onEnableNotifications, onDisableNotifications, onTestNotification, onCreateWorker, onUpdateWorker, onReorderWorkers, onUpdateWorkerEngine, onReloadDevelopment, onHiveIdentityChange }: Props) {
  const mobile = deviceClass() === "mobile";
  const [terminalHostStatus, setTerminalHostStatus] = useState<TerminalHostStatus>();
  const [terminalHostLoaded, setTerminalHostLoaded] = useState(false);
  const developmentRuntime = useDevelopmentRuntime(operatorToken, health?.version);
  const [jiraReadiness, setJiraReadiness] = useState<JiraReadiness>();
  const [jiraUnavailable, setJiraUnavailable] = useState(false);
  const [emailReadiness, setEmailReadiness] = useState<EmailReadiness>();
  const [emailUnavailable, setEmailUnavailable] = useState(false);
  const [confirmMaintenance, setConfirmMaintenance] = useState(false);
  const [activeSettingsSection, setActiveSettingsSection] = useState(() => readSettingsSection() ?? "settings-crew");
  useEffect(() => {
    let frame: number | undefined;
    let settleTimer: number | undefined;
    let finalTimer: number | undefined;
    const scrollToSection = (section: string) => {
      document.getElementById(section)?.scrollIntoView?.({ behavior: "auto", block: "start" });
    };
    const selectLinkedSection = () => {
      const section = readSettingsSection();
      if (!section) return;
      setActiveSettingsSection(section);
      if (frame !== undefined) window.cancelAnimationFrame(frame);
      if (settleTimer !== undefined) window.clearTimeout(settleTimer);
      if (finalTimer !== undefined) window.clearTimeout(finalTimer);
      frame = window.requestAnimationFrame(() => scrollToSection(section));
      settleTimer = window.setTimeout(() => scrollToSection(section), 150);
      finalTimer = window.setTimeout(() => scrollToSection(section), 500);
    };
    selectLinkedSection();
    window.addEventListener("hashchange", selectLinkedSection);
    return () => {
      window.removeEventListener("hashchange", selectLinkedSection);
      if (frame !== undefined) window.cancelAnimationFrame(frame);
      if (settleTimer !== undefined) window.clearTimeout(settleTimer);
      if (finalTimer !== undefined) window.clearTimeout(finalTimer);
    };
  }, []);
  useEffect(() => {
    let cancelled = false;
    setTerminalHostLoaded(false);
    void fetchTerminalHostStatus(operatorToken)
      .then((status) => { if (!cancelled) { setTerminalHostStatus(status); setTerminalHostLoaded(true); } })
      .catch(() => { if (!cancelled) { setTerminalHostStatus(undefined); setTerminalHostLoaded(true); } });
    return () => { cancelled = true; };
  }, [operatorToken, providers]);
  useEffect(() => {
    let cancelled = false;
    void fetchJiraReadiness(operatorToken)
      .then((readiness) => { if (!cancelled) setJiraReadiness(readiness); })
      .catch(() => { if (!cancelled) setJiraUnavailable(true); });
    return () => { cancelled = true; };
  }, [operatorToken]);
  useEffect(() => {
    let cancelled = false;
    void fetchEmailReadiness(operatorToken)
      .then((readiness) => { if (!cancelled) { setEmailReadiness(readiness); setEmailUnavailable(false); } })
      .catch(() => { if (!cancelled) setEmailUnavailable(true); });
    return () => { cancelled = true; };
  }, [operatorToken]);
  async function downloadBackup() {
    const blob = await downloadDatabaseBackup(operatorToken);
    downloadBlob(blob, `swarm-next-hive-${new Date().toISOString().slice(0, 10)}.sqlite3`);
  }
  const workerEngineNeedsUpdate = workerEngineUpdateRequired(health, terminalHostStatus);
  const activeWorkerCount = terminalHostStatus?.running_sessions ?? 0;
  return (
    <div className="settings-workspace">
      <nav className="settings-section-nav" aria-label="Settings sections">
        {SETTINGS_SECTIONS.map(([id, label]) => (
          <button
            key={id}
            type="button"
            aria-controls={id}
            aria-current={activeSettingsSection === id ? "location" : undefined}
            onClick={() => {
              setActiveSettingsSection(id);
              navigateToSettingsSection(id);
              document.getElementById(id)?.scrollIntoView?.({ behavior: "auto", block: "start" });
            }}
          >{label}</button>
        ))}
      </nav>
      <WorkerSettings workers={workers} workspaces={workspaces} busy={busy} providers={providers} onCreate={onCreateWorker} onUpdate={onUpdateWorker} onReorder={onReorderWorkers} />
      <section id="settings-presence" className="settings-card presence-settings" aria-labelledby="presence-heading">
        <div><p className="eyebrow">Presence</p><h3 id="presence-heading">Let attention follow you</h3></div>
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
      <section id="settings-queen" className="settings-card queen-policy-settings" aria-labelledby="queen-policy-heading">
        <div><p className="eyebrow">Queen autonomy</p><h3 id="queen-policy-heading">Choose how far she may carry work</h3></div>
        <p>Presence changes the ceiling for unattended work. These deterministic limits never expand from model confidence.</p>
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
                <option value="coordinate">Coordinate workers</option>
                <option value="local_execution">Allow local execution</option>
              </select>
            </label>
          ))}
        </div>
        <small className="privacy-note">Pushes, deployments, messages, purchases, and other external effects still require a separately recorded approval. Repository and environment overrides come before unattended execution is enabled.</small>
      </section>
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
      <section id="settings-appearance" className="settings-card" aria-labelledby="appearance-heading">
        <div><p className="eyebrow">Appearance</p><h3 id="appearance-heading">Comfortable in long sessions</h3></div>
        <p>Both themes use the same soft natural palette and high-legibility type system. This choice follows your {mobile ? "mobile" : "desktop"} profile and is included in Hive backups.</p>
        <div className="theme-choice" role="group" aria-label="Color theme">
          <button aria-pressed={colorTheme === "light"} onClick={() => onThemeChange("light")}><span className="theme-swatch light" /> Light meadow</button>
          <button aria-pressed={colorTheme === "dark"} onClick={() => onThemeChange("dark")}><span className="theme-swatch dark" /> Night hive</button>
        </div>
      </section>

      <section className="settings-card" aria-labelledby="identity-heading">
        <div><p className="eyebrow">Identity</p><h3 id="identity-heading">Your Hive</h3></div>
        <p>This local boundary owns its workers, tasks, repositories, and provider sessions.</p>
        <dl className="diagnostic-list">
          <div><dt>Hive</dt><dd>{hiveIdentity?.hive.name ?? "Unavailable"}</dd></div>
          <div><dt>Operator</dt><dd>{hiveIdentity?.operator.display_name ?? "Unavailable"}</dd></div>
          <div><dt>Membership</dt><dd>{apiaryMembershipLabel(hiveIdentity)}</dd></div>
        </dl>
      </section>

      <ApiarySettings busy={busy} hiveIdentity={hiveIdentity} operatorToken={operatorToken} onHiveIdentityChange={onHiveIdentityChange} />

      <section id="settings-runtime" className="settings-card" aria-labelledby="runtime-heading">
        <div><p className="eyebrow">Runtime</p><h3 id="runtime-heading">Local system</h3></div>
        <dl className="diagnostic-list runtime-summary-list">
          <div><dt>App/API</dt><dd>{health ? compactRuntimeVersion(health.version) : "Unavailable"}</dd></div>
          <div><dt>Live updates</dt><dd>{liveFeedLabel(liveFeedState)}</dd></div>
          <div><dt>Running workers</dt><dd>{workers.filter((worker) => worker.running).length}</dd></div>
          <div><dt>Retained sessions</dt><dd>{sessions.length}</dd></div>
        </dl>
        <div className="runtime-subsystem-grid">
          <article className={`runtime-subsystem-card ${workerEngineNeedsUpdate ? "runtime-subsystem-restart" : "runtime-subsystem-current"}`} aria-label="Worker engine status">
            <header><div><span className="runtime-component-name">Worker engine</span><strong>{workerEngineLabel(health, terminalHostStatus, terminalHostLoaded)}</strong></div><span className={`runtime-status-badge ${workerEngineNeedsUpdate ? "restart" : "current"}`}>{workerEngineNeedsUpdate ? "Restart required" : "Current"}</span></header>
            {workerEngineNeedsUpdate ? <>
              <p>Updating this layer briefly stops {activeWorkerCount} active worker{activeWorkerCount === 1 ? "" : "s"}, then revives Queen and configured always-active workers from their saved conversations.</p>
              <small><strong>Active commands can be interrupted.</strong> Wait for workers to rest when practical. Identities, provider conversations, tasks, ownership, and terminal history remain durable.</small>
            {!confirmMaintenance ? (
              <button className="secondary-button" disabled={busy} onClick={() => setConfirmMaintenance(true)}>Prepare worker engine update</button>
            ) : (
              <div className="maintenance-confirmation" role="group" aria-label="Confirm worker engine update">
                <strong>Restart {activeWorkerCount} active worker{activeWorkerCount === 1 ? "" : "s"} now?</strong>
                <span>Claude/Codex processes will close. Worker identities, tasks, and known conversation IDs remain durable.</span>
                <div className="settings-actions">
                  <button className="secondary-button" disabled={busy} onClick={() => setConfirmMaintenance(false)}>Not now</button>
                  <button className="primary-action" disabled={busy} onClick={() => { setConfirmMaintenance(false); void onUpdateWorkerEngine(); }}>Stop workers and update</button>
                </div>
              </div>
            )}</> : <><p>The separate terminal host already matches this release. Running workers do not need to restart.</p><small>Claude and Codex processes remain attached to this engine across ordinary app and API reloads.</small></>}
          </article>
          <DevelopmentReloadAction busy={busy} runtime={developmentRuntime} onReload={onReloadDevelopment} />
        </div>
      </section>

      <JiraSettings operatorToken={operatorToken} readiness={jiraReadiness} unavailable={jiraUnavailable} />
      <EmailSettings operatorToken={operatorToken} readiness={emailReadiness} unavailable={emailUnavailable} />

      <section id="settings-backup" className="settings-card" aria-labelledby="backup-heading">
        <div><p className="eyebrow">Backup</p><h3 id="backup-heading">Carry your Hive safely</h3></div>
        <p>Download a consistent snapshot of workers, tasks, conversations, policies, and Hive identity. Repository contents are intentionally excluded.</p>
        <div className="settings-actions">
          <button className="primary-action" disabled={busy} onClick={() => void downloadBackup()}>Download Hive backup</button>
        </div>
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

<DiagnosticsWorkspace feedbackRevision={feedbackRevision} operatorToken={operatorToken} health={health} hiveIdentity={hiveIdentity} liveFeedState={liveFeedState} recentEvents={recentEvents} sessions={sessions} workers={workers} jiraReadiness={jiraReadiness} jiraUnavailable={jiraUnavailable} />

      <section className="settings-card shortcuts-card" aria-labelledby="shortcuts-heading">
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
    </div>
  );
}

function workerEngineLabel(health: Health | undefined, host: TerminalHostStatus | undefined, loaded: boolean) {
  if (!loaded) return "Checking…";
  if (!host) return "Unavailable";
  if (workerEngineUpdateRequired(health, host)) {
    return "Update ready · restart required";
  }
  return `Current · ${host.running_sessions} active`;
}

function compactRuntimeVersion(version: string) {
  const revision = version.match(/-dev-([0-9a-f]{7,40})(?:-|$)/i)?.[1]?.slice(0, 7);
  const release = version.split("-dev-")[0];
  return revision ? `Healthy · ${release} · ${revision}` : `Healthy · ${version}`;
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

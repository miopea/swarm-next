import { downloadDatabaseBackup, type ControlRoomEvent, type Health, type HiveIdentity, type NotificationPolicy, type NotificationSettings, type OperatorPresence, type PresenceMode, type ProviderKind, type QueenAutonomyLevel, type QueenAutonomyPolicy, type SessionSummary, type Worker, type WorkspaceChoice } from "../api";
import type { ColorTheme } from "../brand/theme";
import type { LiveFeedState } from "../controlRoom/ControlRoomLiveFeed";
import type { LockDetectionState } from "../presence/PresenceController";
import { deviceClass } from "../presence/PresenceController";
import type { NotificationCapabilityState } from "../notifications/NotificationController";
import DiagnosticsWorkspace from "./DiagnosticsWorkspace";
import WorkerSettings from "./WorkerSettings";

type Props = {
  busy: boolean;
  colorTheme: ColorTheme;
  health: Health | undefined;
  hiveIdentity: HiveIdentity | undefined;
  liveFeedState: LiveFeedState;
  operatorToken: string;
  presence: OperatorPresence | undefined;
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
  onCreateWorker: (name: string, workspace: string, provider: ProviderKind) => Promise<void>;
  onUpdateWorker: (workerId: string, name: string, autostart: boolean) => Promise<void>;
  onReorderWorkers: (workerIds: string[]) => Promise<void>;
};

export default function SettingsWorkspace({ busy, colorTheme, health, hiveIdentity, liveFeedState, operatorToken, presence, lockDetectionState, notificationSettings, queenPolicy, notificationState, recentEvents, sessions, workers, workspaces, onThemeChange, onPresenceChange, onEnableLockDetection, onNotificationPolicyChange, onQueenPolicyChange, onEnableNotifications, onDisableNotifications, onTestNotification, onCreateWorker, onUpdateWorker, onReorderWorkers }: Props) {
  const mobile = deviceClass() === "mobile";
  async function downloadBackup() {
    const blob = await downloadDatabaseBackup(operatorToken);
    const url = URL.createObjectURL(blob);
    const anchor = document.createElement("a");
    anchor.href = url;
    anchor.download = `swarm-next-hive-${new Date().toISOString().slice(0, 10)}.sqlite3`;
    anchor.click();
    URL.revokeObjectURL(url);
  }
  return (
    <div className="settings-workspace">
      <WorkerSettings workers={workers} workspaces={workspaces} busy={busy} onCreate={onCreateWorker} onUpdate={onUpdateWorker} onReorder={onReorderWorkers} />
      <section className="settings-card presence-settings" aria-labelledby="presence-heading">
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
      <section className="settings-card queen-policy-settings" aria-labelledby="queen-policy-heading">
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
      <section className="settings-card notification-settings" aria-labelledby="notification-heading">
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
            <button className="primary-action" disabled={notificationState === "unsupported" || notificationState === "denied" || notificationState === "enabling" || !notificationSettings} onClick={() => void onEnableNotifications()}>{notificationState === "enabling" ? "Enabling…" : "Enable this device"}</button>
          )}
          <button className="secondary-button" disabled={notificationState !== "enabled"} onClick={() => void onTestNotification()}>Test this device</button>
        </div>
        <small className="privacy-note">Notification text never includes repository names, task titles, evidence, credentials, or terminal output.</small>
      </section>
      <section className="settings-card" aria-labelledby="appearance-heading">
        <div><p className="eyebrow">Appearance</p><h3 id="appearance-heading">Comfortable in long sessions</h3></div>
        <p>Both themes use the same soft natural palette and high-legibility type system.</p>
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
          <div><dt>Membership</dt><dd>{hiveIdentity?.hive.apiary_id ? "Apiary member" : "Personal Hive"}</dd></div>
        </dl>
      </section>

      <section className="settings-card" aria-labelledby="runtime-heading">
        <div><p className="eyebrow">Runtime</p><h3 id="runtime-heading">Local system</h3></div>
        <dl className="diagnostic-list">
          <div><dt>API</dt><dd>{health ? `Healthy · ${health.version}` : "Unavailable"}</dd></div>
          <div><dt>Live updates</dt><dd>{liveFeedLabel(liveFeedState)}</dd></div>
          <div><dt>Running workers</dt><dd>{workers.filter((worker) => worker.running).length}</dd></div>
          <div><dt>Retained sessions</dt><dd>{sessions.length}</dd></div>
          <div><dt>Worker updates</dt><dd>Preserved during API releases</dd></div>
        </dl>
      </section>

      <section className="settings-card integration-settings" aria-labelledby="integration-heading">
        <div><p className="eyebrow">Integrations</p><h3 id="integration-heading">Bring Jira in without making it a bottleneck</h3></div>
        <p>Your Hive stays useful on its own. Jira connections use this operator's identity; credentials and permissions never come from Queen or another Hive.</p>
        <div className="integration-status" role="status">
          <span className="presence waiting" />
          <span><strong>Jira not connected</strong><small>Local workers and private tasks are unaffected</small></span>
        </div>
        <dl className="diagnostic-list">
          <div><dt>Hive projects</dt><dd>Synced by this Hive</dd></div>
          <div><dt>Apiary projects</dt><dd>Require membership and verified access</dd></div>
          <div><dt>Offline work</dt><dd>Owned tasks continue; new shared claims wait</dd></div>
        </dl>
        <small className="privacy-note">Connection setup appears here when the Jira credential adapter is enabled. Swarm stores readiness and mappings, not browser passwords.</small>
      </section>

      <section className="settings-card" aria-labelledby="backup-heading">
        <div><p className="eyebrow">Backup</p><h3 id="backup-heading">Carry your Hive safely</h3></div>
        <p>Download a consistent snapshot of workers, tasks, conversations, policies, and Hive identity. Repository contents are intentionally excluded.</p>
        <div className="settings-actions">
          <button className="primary-action" disabled={busy} onClick={() => void downloadBackup()}>Download Hive backup</button>
        </div>
        <small className="privacy-note">This file contains private operational data. Store it like a credential. Verified restore and full environment configuration export are the next backup checkpoint.</small>
      </section>

<DiagnosticsWorkspace operatorToken={operatorToken} health={health} hiveIdentity={hiveIdentity} liveFeedState={liveFeedState} recentEvents={recentEvents} sessions={sessions} workers={workers} />

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


function notificationStateLabel(state: NotificationCapabilityState) {
  if (state === "enabled") return "Ready on this device";
  if (state === "enabling") return "Enabling this device";
  if (state === "denied") return "Browser permission blocked";
  if (state === "unsupported") return "Push unavailable in this browser";
  if (state === "error") return "Needs another try";
  return "Available when you choose";
}

function notificationStateDetail(state: NotificationCapabilityState, count: number) {
  if (state === "enabled") return `${count} device${count === 1 ? "" : "s"} registered with this Hive`;
  if (state === "enabling") return "Waiting for the browser and Hive to confirm the subscription.";
  if (state === "denied") return "Allow notifications in browser site settings to enable them.";
  if (state === "unsupported") return "Presence and the Needs you inbox continue to work normally.";
  if (state === "error") return "No notification was enabled; retry when the connection is stable.";
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
function liveFeedLabel(state: LiveFeedState) {
  if (state === "connected") return "Connected";
  if (state === "retrying") return "Reconnecting";
  return "Connecting";
}

import type { ControlRoomEvent, Health, HiveIdentity, OperatorPresence, PresenceMode, SessionSummary, Worker } from "../api";
import type { ColorTheme } from "../brand/theme";
import type { LiveFeedState } from "../controlRoom/ControlRoomLiveFeed";
import type { LockDetectionState } from "../presence/PresenceController";
import DiagnosticsWorkspace from "./DiagnosticsWorkspace";

type Props = {
  colorTheme: ColorTheme;
  health: Health | undefined;
  hiveIdentity: HiveIdentity | undefined;
  liveFeedState: LiveFeedState;
  operatorToken: string;
  presence: OperatorPresence | undefined;
  lockDetectionState: LockDetectionState;
  recentEvents: ControlRoomEvent[];
  sessions: SessionSummary[];
  workers: Worker[];
  onThemeChange: (theme: ColorTheme) => void;
  onPresenceChange: (mode: PresenceMode | null) => Promise<void>;
  onEnableLockDetection: () => Promise<void>;
};

export default function SettingsWorkspace({ colorTheme, health, hiveIdentity, liveFeedState, operatorToken, presence, lockDetectionState, recentEvents, sessions, workers, onThemeChange, onPresenceChange, onEnableLockDetection }: Props) {
  return (
    <div className="settings-workspace">
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
        <div className="presence-lock-row">
          <div><strong>Computer lock detection</strong><small>{lockDetectionLabel(lockDetectionState)}</small></div>
          <button className="secondary-button" disabled={lockDetectionState === "unsupported" || lockDetectionState === "enabled"} onClick={() => void onEnableLockDetection()}>
            {lockDetectionState === "enabled" ? "Enabled" : "Enable"}
          </button>
        </div>
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

<DiagnosticsWorkspace operatorToken={operatorToken} health={health} hiveIdentity={hiveIdentity} liveFeedState={liveFeedState} recentEvents={recentEvents} sessions={sessions} workers={workers} />

      <section className="settings-card shortcuts-card" aria-labelledby="shortcuts-heading">
        <div><p className="eyebrow">Keyboard</p><h3 id="shortcuts-heading">Move without losing focus</h3></div>
        <dl className="shortcut-list">
          <div><dt>Tasks</dt><dd><kbd>Alt</kbd><kbd>1</kbd></dd></div>
          <div><dt>Workers</dt><dd><kbd>Alt</kbd><kbd>2</kbd></dd></div>
          <div><dt>Settings</dt><dd><kbd>Alt</kbd><kbd>3</kbd></dd></div>
          <div><dt>Previous / next worker</dt><dd><kbd>Alt</kbd><kbd>↑</kbd> / <kbd>Alt</kbd><kbd>↓</kbd></dd></div>
        </dl>
        <p>Shortcuts pause while you type in a terminal, field, or menu.</p>
      </section>
    </div>
  );
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
  if (state === "available") return "Supported; enabling requires one browser permission.";
  if (state === "denied") return "Not granted; activity and visibility fallback remain active.";
  return "Unavailable in this browser; fallback presence remains active.";
}
function liveFeedLabel(state: LiveFeedState) {
  if (state === "connected") return "Connected";
  if (state === "retrying") return "Reconnecting";
  return "Connecting";
}

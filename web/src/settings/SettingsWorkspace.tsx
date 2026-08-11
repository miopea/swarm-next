import type { Health, HiveIdentity } from "../api";
import type { ColorTheme } from "../brand/theme";
import type { LiveFeedState } from "../controlRoom/ControlRoomLiveFeed";

type Props = {
  colorTheme: ColorTheme;
  health: Health | undefined;
  hiveIdentity: HiveIdentity | undefined;
  liveFeedState: LiveFeedState;
  runningWorkers: number;
  retainedSessions: number;
  onThemeChange: (theme: ColorTheme) => void;
};

export default function SettingsWorkspace({ colorTheme, health, hiveIdentity, liveFeedState, runningWorkers, retainedSessions, onThemeChange }: Props) {
  return (
    <div className="settings-workspace">
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
          <div><dt>Running workers</dt><dd>{runningWorkers}</dd></div>
          <div><dt>Retained sessions</dt><dd>{retainedSessions}</dd></div>
          <div><dt>Worker updates</dt><dd>Preserved during API releases</dd></div>
        </dl>
      </section>

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


function liveFeedLabel(state: LiveFeedState) {
  if (state === "connected") return "Connected";
  if (state === "retrying") return "Reconnecting";
  return "Connecting";
}

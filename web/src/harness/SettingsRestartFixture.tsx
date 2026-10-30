import type { WorkerBehindSettings } from "../api";
import RuntimeUpdateCards from "../runtime/RuntimeUpdateCards";
import { nextRuntimeUpdates } from "../runtime/runtimeUpdates";
import SettingsRestartAction from "../settings/SettingsRestartAction";

/** The fixture Hive's workers behind a setting; `?settings=several` for more than one. */
export function fixtureSettingsRestart(): { workers: WorkerBehindSettings[] } {
  const several = new URLSearchParams(window.location.search).get("settings") === "several";
  return {
    workers: several
      ? [
        { worker_id: "petal", name: "Petal", settings: ["remote_control", "queen_releases"] },
        { worker_id: "thistle", name: "Thistle", settings: ["queen_releases"] },
        { worker_id: "clover", name: "Clover with a much longer worker name", settings: ["system_access", "worker_model"] },
      ]
      : [{ worker_id: "petal", name: "Petal", settings: ["queen_releases"] }],
  };
}

/**
 * The rail's row and the Updates card for workers started before a setting
 * changed (ADR 0132). Nothing is restarted.
 */
export default function SettingsRestartFixture() {
  const rail = nextRuntimeUpdates(undefined, undefined, undefined, undefined, [], fixtureSettingsRestart().workers) ?? [];
  return (
    <div style={{ display: "grid", gap: 16, maxWidth: 760 }}>
      <aside className="control-rail" style={{ width: 280 }}>
        <RuntimeUpdateCards updates={rail} busy={false} onRun={() => undefined} onDetails={() => undefined} />
      </aside>
      <SettingsRestartAction operatorToken="fixture" busy={false} onRestart={() => Promise.resolve()} />
    </div>
  );
}

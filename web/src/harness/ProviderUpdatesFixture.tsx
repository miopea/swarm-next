import type { SupersededProvider } from "../api";
import RuntimeUpdateCards from "../runtime/RuntimeUpdateCards";
import { runtimeUpdates } from "../runtime/runtimeUpdates";
import ProviderReleaseAction from "../settings/ProviderReleaseAction";

/**
 * The Updates card and the rail's row for a Claude update (ADR 0122), in the state ?state=
 * names: "automatic" (Swarm is moving both workers itself), "refused" (Claude refused one), or
 * "mixed" (a Codex worker beside them still needs stopping). Nothing is restarted.
 */
export default function ProviderUpdatesFixture() {
  const state = new URLSearchParams(window.location.search).get("state") ?? "automatic";
  const superseded: SupersededProvider[] = [
    {
      provider: "claude_code",
      version: "2.1.310",
      installed_at: 1_000,
      worker_ids: ["petal", "thistle"],
      in_place_worker_ids: ["petal", "thistle"],
      asked_worker_ids: state === "refused" ? ["thistle"] : [],
    },
    ...(state === "mixed"
      ? [{ provider: "codex" as const, version: null, installed_at: 1_000, worker_ids: ["clover"], in_place_worker_ids: [], asked_worker_ids: [] }]
      : []),
  ];
  const rail = runtimeUpdates(undefined, undefined, undefined, superseded);
  return (
    <div style={{ display: "grid", gap: 16, maxWidth: 760 }}>
      <aside className="control-rail" style={{ width: 280 }}>
        <RuntimeUpdateCards updates={rail} busy={false} onRun={() => undefined} onDetails={() => undefined} />
      </aside>
      <ProviderReleaseAction superseded={superseded} busy={false} onRestart={() => Promise.resolve()} />
    </div>
  );
}

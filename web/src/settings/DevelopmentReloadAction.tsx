import { useState } from "react";
import { deployedRevision, runtimeVersionIdentity, shortRevision } from "./runtimeVersion";

type Props = {
  busy: boolean;
  runtime?: import("../api").DevelopmentRuntime;
  /** Whether the API is currently answering. False while it restarts under a reload. */
  reachable?: boolean;
  healthVersion?: string;
  onReload: () => Promise<void>;
};

export default function DevelopmentReloadAction({ busy, runtime, reachable = true, healthVersion, onReload }: Props) {
  const [confirming, setConfirming] = useState(false);
  if (!runtime?.enabled) return null;
  // Activating a build restarts the API, so this card loses contact in the
  // middle of the operation it is reporting. It holds its place and says so:
  // disappearing at that exact moment reads as the build having destroyed
  // something.
  if (!reachable) {
    return (
      <article className="runtime-subsystem-card runtime-subsystem-safe development-reload-action" aria-label="App and API status" role="status">
        <header><div><span className="runtime-component-name">App and API</span><strong>Waiting for the API to answer</strong></div><span className="runtime-status-badge safe">Workers stay online</span></header>
        <div className="maintenance-progress" aria-live="polite">
          <span className="maintenance-spinner" aria-hidden="true" />
          <div>
            <strong>Reconnecting…</strong>
            <span>The API is not answering right now, which is expected while a new build takes over. This page reconnects on its own.</span>
          </div>
        </div>
        <small>Claude, Codex, and the worker engine are not restarted by an App and API release.</small>
      </article>
    );
  }
  const runningRevision = shortRevision(runtime.deployed_source_revision) ?? deployedRevision(runtime.version);
  const workingRevision = shortRevision(runtime.source_revision) ?? "the working copy";
  // A finished build said nothing about having finished. The card went back to
  // offering a reload — correctly, because newer commits exist — and the
  // operator could not tell whether the build they asked for had landed.
  // The working copy can differ from what is running without being a different
  // commit: uncommitted changes are a reason to rebuild that no revision
  // comparison can show. Naming the same revision on both sides and offering a
  // reload anyway reads as nonsense.
  // THE SECOND MEANING OF "IT IS LIVE", said where the operator already looks.
  // A reload deliberately does not restart the terminal host — that is what
  // stops it killing every worker's terminal mid-turn — so the engine can be
  // left behind by a reload that reports success. The operator met that as
  // `Runtime request returned 422: unknown variant "start_shell"`, which reads
  // as a protocol bug rather than as a service that had not restarted.
  //
  // Rendered on BOTH branches of this card: a Hive whose app matches its
  // checkout can still be running a stale engine, and that is exactly the case
  // where nothing else would mention it.
  // LOUDER THAN THE ENGINE NOTICE, because it is a harder stop. An engine that
  // is behind still lets a reload land; a protocol change means the reload
  // cannot install at all, and the operator would otherwise discover that by
  // pressing the button and being refused.
  const protocolPending = runtime.protocol_migration_required === true ? (
    <p className="runtime-protocol-pending">
      <strong>This checkout changes the terminal-host protocol.</strong> A reload cannot install
      it — a reload leaves the terminal host running, which is what keeps worker terminals alive.
      Installing it swaps the API and the host together and <strong>stops every worker</strong>,
      so run the protocol migration when your workers are idle.
    </p>
  ) : null;
  /**
   * ⚠️ THIS SENTENCE USED TO INVITE AN ACTION A TIMER WAS ALREADY TAKING.
   *
   * It read "so run the worker engine update when your workers are idle" — every
   * clause true, and together an invitation to do something later that a
   * background check does FOR you, on exactly the trigger the sentence names.
   *
   * On 2026-09-03 that cost ninety minutes: a reload at 13:22 left the engine
   * behind, the reconcile timer deferred three times while workers were
   * mid-turn, and swapped at 13:51 the first moment all ten were resting. The
   * operator WAS told the engine was behind. They were never told what would
   * happen next, or that it would happen without them.
   *
   * The count is what makes it weighable — a swap that stops eleven sessions and
   * one that stops none read identically without it. The non-return is what
   * turns a restart into an afternoon: autostart is off on every profile but
   * Queen, deliberately, so the rest wait to be started by hand.
   */
  const runningSessions = runtime.running_worker_sessions;
  const engineBehind = runtime.worker_engine_update_required === true ? (
    <p className="runtime-engine-behind">
      <strong>The worker engine is behind this build, and a background check will install it
      without asking.</strong> It swaps at the first moment no worker is mid-turn — which may be
      minutes or an hour — and that stops{" "}
      {typeof runningSessions === "number"
        ? `all ${runningSessions} running worker session${runningSessions === 1 ? "" : "s"}`
        : "every running worker session"}
      . This build's compatible-engine updater records loaded workers for recovery before the swap.
      Recovery may pause for provider policy or a reported failure; it is not confirmation that
      context was restored. Use the worker engine update below to choose the moment.
    </p>
  ) : null;
  const uncommittedOnly = runtime.source_dirty
    && runtime.source_revision === runtime.deployed_source_revision;
  /**
   * The running code exists only on this machine.
   *
   * A reload builds from the local checkout, which is what makes the
   * develop-and-reload loop work. What was invisible is the consequence: this
   * Hive ran a commit that existed on no remote for about twenty minutes, and
   * both a worker and Queen reported it as "pushed, not deployed" when it was
   * deployed and not pushed. Committed, pushed and deployed are three claims;
   * the surface carried only the third.
   *
   * Reported, not gated — refusing would break the loop this exists to serve.
   */
  const unpublished = runtime.deployed_source_revision && !runtime.deployed_source_published ? (
    <p className="development-build-unpublished" role="status">
      Revision {runningRevision} is running but is not on any remote, so it exists
      only on this machine. Push it to make it recoverable.
    </p>
  ) : null;
  const lastBuildLanded = runtime.state === "ready" ? (
    <p className="development-build-landed" role="status">
      The last build completed, and revision {runningRevision} is serving this page.
    </p>
  ) : null;
  if (runtime.state === "source_mismatch") return (
    <article className="runtime-subsystem-card runtime-subsystem-restart development-reload-action" aria-label="App and API status" role="alert">
      <header><div><span className="runtime-component-name">App and API</span><strong>Development checkout needs to catch up</strong></div><span className="runtime-status-badge restart">Reload blocked</span></header>
      <p>Revision {runningRevision} is active, but working-copy revision {workingRevision} does not contain that deployed source.</p>
      <small>Swarm will not build or activate an older or unrelated checkout. Update the development checkout first; Claude, Codex, and the worker engine remain online.</small>
    </article>
  );
  if (runtime.state === "requested" || runtime.state === "building") {
    // The same inline progress the worker engine card shows. A build that only
    // changes the wording on a card reads as another resting state, which is
    // why this one ran without the operator being able to find it.
    return (
      <article className="runtime-subsystem-card runtime-subsystem-safe development-reload-action" aria-label="App and API status" role="status">
        <header><div><span className="runtime-component-name">App and API</span><strong>Building development changes</strong></div><span className="runtime-status-badge safe">Workers stay online</span></header>
        <div className="maintenance-progress" aria-live="polite">
          <span className="maintenance-spinner" aria-hidden="true" />
          <div>
            <strong>{runtime.state === "requested" ? "Starting the App and API build…" : "Building App and API…"}</strong>
            <span>Compiling and checking {workingRevision}. Revision {runningRevision} keeps serving this page until the new build is healthy.</span>
          </div>
        </div>
        <small>The page reconnects only after the new browser and API build is healthy. Claude, Codex, and the worker engine keep running.</small>
      </article>
    );
  }
  if (runtime.state === "failed") return (
    <article className="runtime-subsystem-card runtime-subsystem-restart development-reload-action" aria-label="App and API status" role="alert">
      <header><div><span className="runtime-component-name">App and API</span><strong>Development build failed</strong></div><span className="runtime-status-badge restart">Current app preserved</span></header>
      <p>Revision {runningRevision} is still serving this page. The attempted {workingRevision} build was rejected before activation.</p>
      <small>Workers were never restarted or interrupted. Check the development reload service log before retrying.</small>
      {runtime.reload_available && !confirming ? <button className="secondary-button" disabled={busy} onClick={() => setConfirming(true)}>Retry development build</button> : null}
      {confirming ? <ReloadConfirmation busy={busy} onCancel={() => setConfirming(false)} onConfirm={() => { setConfirming(false); void onReload(); }} /> : null}
    </article>
  );
  if (!runtime.reload_available) {
    return <article className="runtime-subsystem-card runtime-subsystem-current development-reload-action" aria-label="App and API status"><header><div><span className="runtime-component-name">App and API</span><strong>Running build matches the working copy</strong></div><span className="runtime-status-badge current">Current</span></header><p className="runtime-version"><strong>Installed</strong> {runtimeVersionIdentity(healthVersion ?? runtime.version)}</p><p>Active revision {runningRevision} matches the product code in this checkout. No App/API build is waiting.</p>{unpublished}{protocolPending}{engineBehind}<small>Swarm checks the working copy every 15 seconds. When product code changes, you can build and activate it without restarting Claude, Codex, or the worker engine.</small></article>;
  }
  return (
    <article className="runtime-subsystem-card runtime-subsystem-safe development-reload-action" aria-label="App and API status">
      <header><div><span className="runtime-component-name">App and API</span><strong>Development reload available</strong></div><span className="runtime-status-badge safe">Workers stay online</span></header>
      {lastBuildLanded}{unpublished}{protocolPending}{engineBehind}
      <p>{uncommittedOnly
        ? <>Revision {runningRevision} is active, and the working copy has uncommitted changes on top of it. Building picks those up.</>
        : <>Revision {runningRevision} is active. Build and switch the browser and API to working-copy revision {workingRevision}.</>}</p>
      <small>A failed compile is rejected and {runningRevision} remains active. Claude, Codex, and the worker engine are not restarted.</small>
      {!confirming ? (
        <button className="secondary-button" disabled={busy} onClick={() => setConfirming(true)}>Reload development build</button>
      ) : (
        <ReloadConfirmation busy={busy} onCancel={() => setConfirming(false)} onConfirm={() => { setConfirming(false); void onReload(); }} />
      )}
    </article>
  );
}

function ReloadConfirmation({ busy, onCancel, onConfirm }: { busy: boolean; onCancel: () => void; onConfirm: () => void }) {
  return <div className="maintenance-confirmation" role="group" aria-label="Confirm development reload">
    <strong>Build and activate the working copy?</strong>
    <span>This can take several minutes. Swarm reopens only after the new API and browser build are healthy; the separate worker engine is not restarted.</span>
    <div className="settings-actions">
      <button className="secondary-button" disabled={busy} onClick={onCancel}>Not now</button>
      <button className="primary-action" disabled={busy} onClick={onConfirm}>Build and reload</button>
    </div>
  </div>;
}


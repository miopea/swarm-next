import type { DevelopmentRuntime, Health, SupersededProvider, TerminalHostStatus } from "../api";
import { workerEngineUpdateRequired } from "./workerEngine";

export type RuntimeUpdateKind = "none" | "building" | "failed" | "app" | "worker_engine" | "provider";

export type RuntimeUpdateSummary = {
  kind: RuntimeUpdateKind;
  /** Short label for the control-room indicator. */
  label: string;
  /** The longer explanation, for a title and the accessible name. */
  detail: string;
  /** Whether this is work in progress rather than something waiting on the operator. */
  busy: boolean;
};

const IDLE: RuntimeUpdateSummary = {
  kind: "none",
  label: "",
  detail: "",
  busy: false,
};

/**
 * What the control room should say about pending or in-flight runtime updates.
 *
 * Work in progress outranks work waiting, because an operator watching a build
 * needs to know it is still going more than they need to be told again that an
 * update exists. A failed build outranks both: it is the only state that has
 * stopped making progress on its own.
 *
 * The worker engine is reported separately from App and API because replacing
 * it interrupts running workers, while an App/API release does not. A worker
 * engine replacement that is under way outranks everything, for the same
 * reason: it is the only update that takes workers away while it runs.
 *
 * Both subsystems are named here exactly as the settings page names them.
 * The indicator leads to that page, so a word that appears in one and not the
 * other sends the operator looking for something that is not there.
 */
export function runtimeUpdateSummary(
  health: Health | undefined,
  host: TerminalHostStatus | undefined,
  development: DevelopmentRuntime | undefined,
  superseded: SupersededProvider[] = [],
): RuntimeUpdateSummary {
  if (host?.draining) {
    return {
      kind: "worker_engine",
      label: "Updating worker engine",
      detail: "The worker engine is being replaced. The workers it unloaded are brought back once it reports in.",
      busy: true,
    };
  }

  if (development?.state === "failed") {
    return {
      kind: "failed",
      label: "App and API build failed",
      detail: "The development working copy did not compile. The current release is still running.",
      busy: false,
    };
  }

  if (development?.state === "building" || development?.state === "requested") {
    const revision = development.source_revision?.slice(0, 7);
    return {
      kind: "building",
      label: "Updating App and API",
      detail: revision
        ? `Building and checking revision ${revision}. Workers keep running.`
        : "Building and checking the development update. Workers keep running.",
      busy: true,
    };
  }

  if (workerEngineUpdateRequired(health, host)) {
    return {
      kind: "worker_engine",
      label: "Worker engine update",
      detail: "A worker engine update is installed but not running. Applying it restarts loaded workers.",
      busy: false,
    };
  }

  // Uncommitted changes at the same revision are work in progress, not an
  // update waiting to be applied. The settings card still offers the build;
  // the indicator does not nag about it, because it would never go quiet
  // while anyone is editing the checkout.
  const onlyUncommitted = development?.source_dirty
    && development.source_revision === development.deployed_source_revision;
  // Ranked below the worker engine and above App and API: like an engine
  // replacement it needs a restart to take effect, and unlike an App and API
  // release it is not running anywhere until each worker restarts.
  if (superseded.length > 0) {
    const workers = new Set(superseded.flatMap((entry) => entry.worker_ids)).size;
    const named = superseded
      .map((entry) => `${entry.provider === "codex" ? "Codex" : "Claude"}${entry.version ? ` ${entry.version}` : ""}`)
      .join(" and ");
    return {
      kind: "provider",
      label: "Provider update",
      detail: `${named} is installed, and ${workers} running worker${workers === 1 ? "" : "s"} started before that, so ${workers === 1 ? "it is" : "they are"} still running the older release.`,
      busy: false,
    };
  }

  if (development?.enabled && development.reload_available && !onlyUncommitted) {
    const revision = development.source_revision?.slice(0, 7);
    return {
      kind: "app",
      label: "App and API update",
      detail: revision
        ? `Revision ${revision} is ready to build. Workers stay online through an App and API release.`
        : "A development update is ready to build. Workers stay online through an App and API release.",
      busy: false,
    };
  }

  return IDLE;
}

/**
 * The summary to show after a refresh, given what that refresh managed to learn.
 *
 * A refresh that learned nothing about any subsystem keeps the previous answer
 * rather than reporting silence as "nothing to update". The App and API build
 * restarts the API, so a refresh returning nothing is the expected middle of
 * the operation the indicator is reporting — the same reason the settings card
 * holds its place instead of disappearing.
 */
export function nextRuntimeUpdate(
  previous: RuntimeUpdateSummary | undefined,
  health: Health | undefined,
  host: TerminalHostStatus | undefined,
  development: DevelopmentRuntime | undefined,
  superseded: SupersededProvider[] = [],
): RuntimeUpdateSummary | undefined {
  if (!health && !host && !development) return previous;
  return runtimeUpdateSummary(health, host, development, superseded);
}

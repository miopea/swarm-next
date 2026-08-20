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
function workerEngineUpdate(
  health: Health | undefined,
  host: TerminalHostStatus | undefined,
): RuntimeUpdateSummary | undefined {
  if (host?.draining) {
    return {
      kind: "worker_engine",
      label: "Updating worker engine",
      detail: "The worker engine is being replaced. The workers it unloaded are brought back once it reports in.",
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
  return undefined;
}

function providerUpdate(superseded: SupersededProvider[]): RuntimeUpdateSummary | undefined {
  if (superseded.length === 0) return undefined;
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

function appUpdate(development: DevelopmentRuntime | undefined): RuntimeUpdateSummary | undefined {
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
  // Uncommitted changes at the same revision are work in progress, not an
  // update waiting to be applied. The settings card still offers the build;
  // the indicator does not nag about it, because it would never go quiet
  // while anyone is editing the checkout.
  const onlyUncommitted = development?.source_dirty
    && development.source_revision === development.deployed_source_revision;
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
  return undefined;
}

/**
 * Everything the control room should currently say about runtime updates.
 *
 * One entry per subsystem, in the order they cost the operator: the worker
 * engine takes workers away, a provider update is installed and running
 * nowhere until each worker restarts, and an App and API release leaves
 * workers online throughout.
 *
 * They are reported together rather than ranked into one, because they are
 * independent and can all be true — showing only the most severe hid the
 * others until it was dealt with. This mirrors the settings page, which has
 * a card for each, and uses the same names it does.
 */
export function runtimeUpdates(
  health: Health | undefined,
  host: TerminalHostStatus | undefined,
  development: DevelopmentRuntime | undefined,
  superseded: SupersededProvider[] = [],
): RuntimeUpdateSummary[] {
  return [
    workerEngineUpdate(health, host),
    providerUpdate(superseded),
    appUpdate(development),
  ].filter((entry): entry is RuntimeUpdateSummary => entry !== undefined);
}

/** The single most pressing update, when only one can be shown. */
export function runtimeUpdateSummary(
  health: Health | undefined,
  host: TerminalHostStatus | undefined,
  development: DevelopmentRuntime | undefined,
  superseded: SupersededProvider[] = [],
): RuntimeUpdateSummary {
  return runtimeUpdates(health, host, development, superseded)[0] ?? IDLE;
}

export function nextRuntimeUpdates(
  previous: RuntimeUpdateSummary[] | undefined,
  health: Health | undefined,
  host: TerminalHostStatus | undefined,
  development: DevelopmentRuntime | undefined,
  superseded: SupersededProvider[] = [],
): RuntimeUpdateSummary[] | undefined {
  if (!health && !host && !development) return previous;
  return runtimeUpdates(health, host, development, superseded);
}

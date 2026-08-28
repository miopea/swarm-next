import type { DevelopmentRuntime, Health, SupersededProvider, TerminalHostStatus } from "../api";
import { workerEngineUpdateRequired } from "./workerEngine";

export type RuntimeUpdateKind = "none" | "building" | "failed" | "app" | "worker_engine" | "provider";

/** What running this update actually does, when the operator asks for it here. */
export type RuntimeUpdateAction = "build" | "apply_worker_engine" | "restart_providers";

export type RuntimeUpdateSummary = {
  kind: RuntimeUpdateKind;
  /** Short label for the control-room indicator. */
  label: string;
  /** The longer explanation. Shown, not only hovered: a title attribute is
      nothing on a phone, and this is the part that says what is happening. */
  detail: string;
  /** Whether this is work in progress rather than something waiting on the operator. */
  busy: boolean;
  /** Absent while busy: there is nothing to start that is not already running. */
  action?: RuntimeUpdateAction;
  /** What the operator is asked to press. */
  actionLabel?: string;
  /** Set when running this takes workers away. Drives the stronger warning. */
  consequence?: string;
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
  // A PREPARED PROTOCOL MIGRATION, waiting for the workers to go quiet.
  //
  // Checked before the engine update because a migration swaps the engine as
  // part of its own atomic step: reporting an engine update here would offer
  // an action that moves host-current out from under the migration.
  //
  // It applies itself within two minutes of the last worker going idle. This
  // exists so the operator does not have to wait for that, and it carries the
  // same consequence as the engine update because it costs the same thing.
  if (
    health?.protocol_migration_pending !== undefined
    && host !== undefined
    && health.protocol_migration_pending !== host.protocol_version
  ) {
    return {
      kind: "worker_engine",
      label: "Protocol migration ready",
      detail: host.running_sessions > 0
        ? `An update that changes how Swarm talks to its worker engine is installed and waiting. It applies itself once your ${host.running_sessions} running worker${host.running_sessions === 1 ? "" : "s"} ${host.running_sessions === 1 ? "is" : "are"} idle, or you can apply it now.`
        : "An update that changes how Swarm talks to its worker engine is installed and waiting. Nothing is running, so it applies within two minutes — or you can apply it now.",
      busy: false,
      action: "apply_worker_engine",
      actionLabel: "Apply the protocol migration",
      consequence: "Every loaded worker is stopped and brought back. Work in a terminal that has not been saved is lost, and each worker reconnects on the new engine.",
    };
  }
  if (workerEngineUpdateRequired(health, host)) {
    return {
      kind: "worker_engine",
      label: "Worker engine update",
      detail: "A worker engine update is installed but not running. Applying it restarts loaded workers.",
      busy: false,
      action: "apply_worker_engine",
      actionLabel: "Apply worker engine update",
      consequence: "Every loaded worker is stopped and brought back. Work in a terminal that has not been saved is lost, and each worker reconnects on the new engine.",
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
    action: "restart_providers",
    actionLabel: "Restart onto the new release",
    consequence: `${workers} running worker${workers === 1 ? "" : "s"} ${workers === 1 ? "is" : "are"} restarted. Each loses its current conversation and starts again on the newer release.`,
  };
}

function appUpdate(development: DevelopmentRuntime | undefined): RuntimeUpdateSummary | undefined {
  // A build that stopped making progress, and a development mode configured to
  // write somewhere that does not exist. Both used to read as "nothing is
  // happening", which is what left a build apparently running with nothing
  // behind it.
  if (development?.state === "stalled") {
    return {
      kind: "failed",
      label: "Build stopped responding",
      detail: "The build has made no progress for some time. Nothing is compiling; it was most likely never picked up.",
      busy: false,
      action: "build",
      actionLabel: "Start it again",
    };
  }
  if (development?.state === "unavailable") {
    return {
      kind: "failed",
      label: "Development mode is misconfigured",
      detail: "This Hive is set to build from a working copy, but the path it reports progress to does not exist. Re-enable development mode to repair it.",
      busy: false,
    };
  }
  if (development?.state === "failed") {
    return {
      kind: "failed",
      label: "App and API build failed",
      // Repeats what the reload recorded rather than asserting a cause. The
      // fixed sentence here claimed a compiler error for every failure,
      // including installs that were refused after compiling cleanly.
      detail: development.failure_detail?.trim()
        ? `${developmentFailureHeadline(development.failure_reason)} It said: ${development.failure_detail.trim()}`
        : developmentFailureHeadline(development.failure_reason),
      busy: false,
      action: "build",
      actionLabel: "Build again",
    };
  }
  if (development?.state === "building" || development?.state === "requested") {
    const revision = development.source_revision?.slice(0, 7);
    return {
      kind: "building",
      label: "Updating App and API",
      detail: revision
        ? `Building and checking revision ${revision}. Workers keep running and Swarm stays usable; the page reloads itself when it is ready.`
        : "Building and checking the development update. Workers keep running and Swarm stays usable; the page reloads itself when it is ready.",
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
      action: "build",
      actionLabel: "Build and release",
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

/** One sentence for the step that failed, and none invented when it is unknown. */
function developmentFailureHeadline(reason?: string | null): string {
  switch (reason) {
    case "build":
      return "The development working copy did not compile. The current release is still running.";
    case "install":
      return "The development build compiled, but could not be installed. The current release is still running.";
    case "protocol-change":
      return "This checkout changes the terminal-host protocol, which a reload cannot install.";
    default:
      return "The development reload failed and did not record why. The current release is still running.";
  }
}

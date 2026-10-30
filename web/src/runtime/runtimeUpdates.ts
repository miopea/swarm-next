import type { DevelopmentRuntime, Health, RestartBoundSetting, SupersededProvider, TerminalHostStatus, WorkerBehindSettings } from "../api";
import { ENGINE_RESTART_CONSEQUENCE, workerEngineUpdateRequired } from "./workerEngine";

export type RuntimeUpdateKind = "none" | "building" | "failed" | "app" | "worker_engine" | "provider" | "worker_settings";

/** What running this update actually does, when the operator asks for it here. */
export type RuntimeUpdateAction = "build" | "apply_worker_engine" | "restart_providers" | "restart_for_settings";

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
  /** The same cost in a few words, for the rail's one-line row. */
  costShort?: string;
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
        ? `The update is prepared, not installed. Your ${host.running_sessions} loaded worker${host.running_sessions === 1 ? "" : "s"} remain running. Use the warned maintenance action when ready; a resting terminal does not authorize automatic replacement.`
        : "The update is prepared, not installed. No worker sessions remain, so the background updater can apply it; you can also apply it now.",
      busy: false,
      action: "apply_worker_engine",
      actionLabel: "Apply the protocol migration",
      consequence: ENGINE_RESTART_CONSEQUENCE,
      costShort: "Restarts loaded workers",
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
      consequence: ENGINE_RESTART_CONSEQUENCE,
      costShort: "Restarts loaded workers",
    };
  }
  return undefined;
}

/**
 * How the workers behind an installed provider release split (ADR 0122): the Claude workers
 * Swarm moves onto it with /restart by itself, the ones of those Claude already refused, and
 * the rest, which only stopping and starting can move.
 */
export function providerUpdateCounts(superseded: SupersededProvider[]) {
  const behind = new Set(superseded.flatMap((entry) => entry.worker_ids));
  const inPlace = new Set(superseded.flatMap((entry) => entry.in_place_worker_ids));
  const refused = new Set(superseded.flatMap((entry) => entry.asked_worker_ids));
  return {
    behind: behind.size,
    inPlace: inPlace.size,
    refused: refused.size,
    stopAndStart: [...behind].filter((worker) => !inPlace.has(worker)).length,
  };
}

// The alpha three are listed for exhaustiveness, not because they can appear
// here: nothing probes their releases, so the API never reports one superseded.
// Naming them keeps this a total map, so adding a provider is a type error
// rather than a blank label.
const PROVIDER_NAME: Record<SupersededProvider["provider"], string> = {
  claude_code: "Claude",
  codex: "Codex",
  gemini: "Gemini",
  grok: "Grok",
  opencode: "OpenCode",
};

/** "Claude 2.1.310 and Codex": one name for the rail and the settings card alike. */
export function supersededReleaseNames(superseded: SupersededProvider[]): string {
  return superseded
    .map((entry) => `${PROVIDER_NAME[entry.provider]}${entry.version ? ` ${entry.version}` : ""}`)
    .join(" and ");
}

function workers(count: number): string {
  return `${count} worker${count === 1 ? "" : "s"}`;
}

function runningWorkers(count: number): string {
  return `${count} running worker${count === 1 ? "" : "s"}`;
}

/**
 * A provider update asks the operator for something only when Swarm cannot do it alone.
 * Moving Claude workers in place needs no button: they go one at a time, each as it comes
 * free, and their conversations carry on (ADR 0122).
 *
 * ⚠️ ONE LINE, NOT A SPINNER CARD, though it is work in progress. A worker can stay busy for
 * hours, and a full card held that long in the rail is what left the worker list at nothing
 * on 2026-10-12. `busy` would buy the spinner and cost the list.
 */
function providerUpdate(superseded: SupersededProvider[]): RuntimeUpdateSummary | undefined {
  if (superseded.length === 0) return undefined;
  const counts = providerUpdateCounts(superseded);
  const named = supersededReleaseNames(superseded);
  if (counts.stopAndStart > 0) {
    const others = counts.inPlace > 0
      ? ` Swarm moves the other ${workers(counts.inPlace)} onto it by itself.`
      : "";
    return {
      kind: "provider",
      label: "Provider update",
      detail: `${named} is installed, and ${runningWorkers(counts.stopAndStart)} started before that, so ${counts.stopAndStart === 1 ? "it is" : "they are"} still running the older release.${others}`,
      busy: false,
      action: "restart_providers",
      actionLabel: "Restart onto the new release",
      consequence: `${runningWorkers(counts.stopAndStart)} ${counts.stopAndStart === 1 ? "is" : "are"} restarted. Swarm attempts to resume their saved conversations on the newer release. Running work is interrupted; unsent terminal input may be lost.`,
      costShort: `Restarts ${runningWorkers(counts.stopAndStart)}`,
    };
  }
  if (counts.refused > 0) {
    return {
      kind: "provider",
      label: "Provider update",
      detail: `${named} is installed. Claude did not restart ${workers(counts.refused)} onto it. It refuses while Remote Control is connected or background work is running.`,
      busy: false,
      action: "restart_providers",
      actionLabel: "Retry the restart",
    };
  }
  return {
    kind: "provider",
    label: "Updating workers",
    detail: `${named} is installed. Swarm is moving ${workers(counts.inPlace)} onto it, each as soon as it is free, and their conversations carry on.`,
    busy: false,
    costShort: `Automatic, as ${counts.inPlace === 1 ? "it comes" : "each comes"} free`,
  };
}

// The names the Settings cards use, in the order the API reports them.
const SETTING_NAME: Record<RestartBoundSetting, string> = {
  worker_model: "Worker model",
  remote_control: "Remote Control",
  queen_releases: "Queen releases",
  system_access: "System access",
};

function listed(names: string[]): string {
  return names.length <= 1 ? names.join("") : `${names.slice(0, -1).join(", ")} and ${names.at(-1)}`;
}

/** "Queen releases and Remote Control": the settings any of these workers are behind on. */
export function settingsBehindNames(behind: WorkerBehindSettings[]): string {
  const settings = new Set(behind.flatMap((worker) => worker.settings));
  return listed((Object.keys(SETTING_NAME) as RestartBoundSetting[])
    .filter((setting) => settings.has(setting))
    .map((setting) => SETTING_NAME[setting]));
}

/** "Queen releases": one worker's settings that are waiting on its restart. */
export function settingNames(settings: RestartBoundSetting[]): string {
  return listed(settings.map((setting) => SETTING_NAME[setting]));
}

/**
 * A setting that reaches a worker only when it next starts, changed while it
 * was running (ADR 0132). The operator turned on Queen releases moments after
 * restarting every worker, and nothing said the workers were still without it.
 *
 * One line in the rail, like a pending provider update: it waits on the
 * operator choosing a moment to interrupt workers, not on anything happening.
 */
function settingsRestartUpdate(behind: WorkerBehindSettings[]): RuntimeUpdateSummary | undefined {
  if (behind.length === 0) return undefined;
  const count = behind.length;
  const named = settingsBehindNames(behind);
  const single = new Set(behind.flatMap((worker) => worker.settings)).size === 1;
  return {
    kind: "worker_settings",
    label: single ? `${named} needs a restart` : "Settings need a restart",
    detail: `${named} changed after ${runningWorkers(count)} started: ${listed(behind.map((worker) => worker.name))}. ${count === 1 ? "It keeps" : "They keep"} the old setting until ${count === 1 ? "it restarts" : "they restart"}.`,
    busy: false,
    action: "restart_for_settings",
    actionLabel: "Restart to apply the change",
    consequence: `${runningWorkers(count)} ${count === 1 ? "is" : "are"} restarted. Swarm attempts to resume their saved conversations with the new settings. Running work is interrupted; unsent terminal input may be lost. Workers already on the current settings are left alone.`,
    costShort: `Restarts ${runningWorkers(count)}`,
  };
}

function appUpdate(development: DevelopmentRuntime | undefined): RuntimeUpdateSummary | undefined {
  if (development?.state === "deferred") {
    return {
      kind: "worker_engine",
      label: "Build waiting for engine maintenance",
      detail: "The build is prepared, not installed. The current app and workers remain running; use the warned worker engine maintenance action when ready.",
      busy: false,
    };
  }
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
        ? `${developmentFailureHeadline(development.failure_reason, true)} It said: ${development.failure_detail.trim()}`
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
        ? `Building and checking revision ${revision}. Swarm stays usable while the build runs. Other open tabs may need reloading afterward.`
        : "Building and checking the development update. Swarm stays usable while the build runs. Other open tabs may need reloading afterward.",
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
  superseded: SupersededProvider[] | undefined = undefined,
  settingsBehind: WorkerBehindSettings[] | undefined = undefined,
): RuntimeUpdateSummary[] | undefined {
  // A failed provider read is not evidence that its pending update disappeared.
  // Retain only one notice, without an action based on an unverified worker set.
  const provider = superseded === undefined
    ? previous?.some((entry) => entry.kind === "provider")
      ? {
        kind: "provider" as const,
        label: "Provider update status unavailable",
        detail: "A provider update was previously reported. Its current status could not be checked. Swarm will check again before offering a restart.",
        busy: false,
      }
      : undefined
    : providerUpdate(superseded);
  // The same for workers behind a setting: no restart is offered for a list
  // that could not be read.
  const settings = settingsBehind === undefined
    ? previous?.some((entry) => entry.kind === "worker_settings")
      ? {
        kind: "worker_settings" as const,
        label: "Settings restart status unavailable",
        detail: "Some workers were running with an older setting. Whether they still are could not be checked. Swarm will check again before offering a restart.",
        busy: false,
      }
      : undefined
    : settingsRestartUpdate(settingsBehind);
  const others = !health && !host && !development
    ? previous?.filter((entry) => entry.kind !== "provider" && entry.kind !== "worker_settings")
    : runtimeUpdates(health, host, development);
  if (!others && !provider && !settings && superseded === undefined && settingsBehind === undefined) return previous;
  const entries = others ?? [];
  const engineIndex = entries.findIndex((entry) => entry.kind !== "worker_engine");
  if (provider) entries.splice(engineIndex < 0 ? entries.length : engineIndex, 0, provider);
  // After the engine and the provider, before an App and API release: it
  // restarts workers, which an App and API release does not.
  const settingsIndex = entries.findIndex((entry) => entry.kind !== "worker_engine" && entry.kind !== "provider");
  if (settings) entries.splice(settingsIndex < 0 ? entries.length : settingsIndex, 0, settings);
  return entries;
}

/** One sentence for the step that failed, and none invented when it is unknown. */
function developmentFailureHeadline(reason?: string | null, hasDetail = false): string {
  switch (reason) {
    case "build":
      return "The development working copy did not compile. The current release is still running.";
    case "install":
      return "The development build compiled, but could not be installed. The current release is still running.";
    case "protocol-change":
      return "This checkout changes the terminal-host protocol, which a reload cannot install.";
    case "source-moved":
      return "The checkout changed while the build was running, so nothing was installed and the current release is still running.";
    default:
      // See App.tsx: a step this function has not been taught about is still a
      // step that recorded its cause, and claiming otherwise contradicts the
      // detail printed beside it.
      return hasDetail
        ? "The development reload failed. The current release is still running."
        : "The development reload failed and did not record why. The current release is still running.";
  }
}

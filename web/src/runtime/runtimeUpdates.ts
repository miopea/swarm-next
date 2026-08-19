import type { DevelopmentRuntime, Health, TerminalHostStatus } from "../api";
import { workerEngineUpdateRequired } from "./workerEngine";

export type RuntimeUpdateKind = "none" | "building" | "failed" | "app" | "worker_engine";

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
 * it interrupts running workers, while an App/API release does not.
 */
export function runtimeUpdateSummary(
  health: Health | undefined,
  host: TerminalHostStatus | undefined,
  development: DevelopmentRuntime | undefined,
): RuntimeUpdateSummary {
  if (development?.state === "failed") {
    return {
      kind: "failed",
      label: "Update failed",
      detail: "The development working copy did not compile. The current release is still running.",
      busy: false,
    };
  }

  if (development?.state === "building" || development?.state === "requested") {
    const revision = development.source_revision?.slice(0, 7);
    return {
      kind: "building",
      label: "Updating",
      detail: revision
        ? `Building and checking revision ${revision}. Workers keep running.`
        : "Building and checking the development update. Workers keep running.",
      busy: true,
    };
  }

  if (workerEngineUpdateRequired(health, host)) {
    return {
      kind: "worker_engine",
      label: "Engine update",
      detail: "A worker engine update is installed but not running. Applying it restarts loaded workers.",
      busy: false,
    };
  }

  if (development?.enabled && development.reload_available) {
    const revision = development.source_revision?.slice(0, 7);
    return {
      kind: "app",
      label: "Update ready",
      detail: revision
        ? `Revision ${revision} is ready to build. Workers stay online through an App and API release.`
        : "A development update is ready to build. Workers stay online through an App and API release.",
      busy: false,
    };
  }

  return IDLE;
}

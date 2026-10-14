import type { PolicyGap } from "../api";
import { queenAutonomyLabel } from "../orchestration/queenAutonomyPresentation";

/**
 * What each requirable setting is called on screen, and what its values mean.
 *
 * Keyed by the setting the domain names (swarm_domain::RequiredSetting); a key
 * this release does not know is shown by its raw name rather than dropped, so a
 * newer Keeper's requirement is never invisible on an older screen.
 */
const LABELS: Record<string, { name: string; values: Record<string, string> }> = {
  "release.auto_install": {
    name: "Ordinary releases install automatically",
    values: { on: "Required on", off: "Required off" },
  },
  "shared_work.minimum_version": {
    name: "Minimum version for shared work",
    values: {},
  },
  "checks.gate": {
    name: "Check gate",
    values: { off: "Off", warn: "Show CI", require: "Require passing CI" },
  },
  "night_watch.hours": {
    name: "Night Watch hours",
    values: {},
  },
  ...Object.fromEntries(
    ([["at_hive", "At the Hive"], ["away", "Reachable"], ["night_watch", "Night Watch"]] as const).map(([tier, presence]) => [
      `queen.autonomy.${tier}`,
      {
        name: `Queen autonomy: ${presence}`,
        values: Object.fromEntries((["advisory", "coordinate", "local_execution"] as const).map((level) => [level, queenAutonomyLabel(level)])),
      },
    ]),
  ),
};

/** How a version setting's value reads. */
const VERSION_KEYS = new Set(["shared_work.minimum_version"]);
/** Settings whose value is a daily window, "22:00-07:00". */
const HOURS_KEYS = new Set(["night_watch.hours"]);

export function settingName(key: string): string {
  return LABELS[key]?.name ?? key;
}

export function settingValue(key: string, value: string): string {
  if (VERSION_KEYS.has(key)) return `${value} or newer`;
  if (HOURS_KEYS.has(key)) return `${value.replace("-", "–")} in each Hive's own time`;
  return LABELS[key]?.values[value] ?? value;
}

/** Why a required setting is not in effect on a Hive, which is also what fixes it. */
export function gapReason(gap: PolicyGap): string {
  switch (gap.reason) {
    case "not_reported": return "This Hive hasn't reported to the Keeper yet";
    case "not_received": return "This Hive hasn't picked up the current policy yet";
    case "release_cannot_apply": return "This Hive's release can't apply it; update it";
    case "needs_local_choice": return gap.key === "night_watch.hours"
      ? "This Hive's operator hasn't chosen its time zone for Night Watch yet"
      : "This Hive's operator has a choice to make before it applies";
    case "different": return `This Hive runs "${gap.local}" instead`;
    case "below_minimum": return `This Hive runs ${gap.running}, so it can't take Apiary tasks until it updates`;
  }
}

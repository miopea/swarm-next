import { useMemo, useState } from "react";
import { requirePolicySettings, type KeeperPolicy } from "../api";
import { settingName, settingValue } from "./policyLabels";

type Props = {
  operatorToken: string;
  /** Read with the rest of the Keeper page, so it recovers when the page does. */
  policy: KeeperPolicy;
  onSaved: (policy: KeeperPolicy) => void;
};

/** The value a setting has in the editor; "" means each Hive decides. */
type Draft = Record<string, string>;

function draftOf(policy: KeeperPolicy): Draft {
  return Object.fromEntries(policy.settings.map((setting) => [setting.key, setting.value]));
}

/** A release number, as the Keeper accepts one: no development builds. */
const RELEASE_NUMBER = /^\d+\.\d+\.\d+$/;
/** A daily window as the Keeper accepts one: "22:00-07:00", two different times. */
const DAILY_HOURS = /^([01]\d|2[0-3]):[0-5]\d-([01]\d|2[0-3]):[0-5]\d$/;

function validHours(value: string) {
  if (!DAILY_HOURS.test(value)) return false;
  const [start, end] = value.split("-");
  return start !== end;
}

function valueOf(draft: Draft, key: string): string {
  return (draft[key] ?? "").trim();
}

/**
 * What every Hive in this Apiary must run (ADR 0113).
 *
 * ⚠️ ALWAYS REQUIRED. The operator ruled on 2026-10-12 that an Apiary setting is
 * locked on every member, with no local override, so this says so before it
 * says anything else — a Keeper choosing a value is choosing it for other
 * people's machines, and a member's only way out is to leave.
 */
export default function ApiaryPolicyPanel({ operatorToken, policy, onSaved }: Props) {
  const [draft, setDraft] = useState<Draft>(() => draftOf(policy));
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string>();
  // A newer revision read from the Keeper replaces the editor's starting point.
  // Keyed to the revision rather than the object, which every page refresh
  // replaces: an unsaved choice must survive the refresh that happens under it.
  const [seenRevision, setSeenRevision] = useState(policy.revision);
  if (policy.revision !== seenRevision) {
    setSeenRevision(policy.revision);
    setDraft(draftOf(policy));
  }

  const dirty = useMemo(() => {
    const saved = draftOf(policy);
    return policy.supported.some((setting) => valueOf(draft, setting.key) !== (saved[setting.key] ?? ""));
  }, [draft, policy]);
  const invalid = policy.supported.some((setting) => {
    const value = valueOf(draft, setting.key);
    if (value === "") return false;
    if (setting.kind === "version") return !RELEASE_NUMBER.test(value);
    if (setting.kind === "hours") return !validHours(value);
    return false;
  });

  async function save() {
    setSaving(true);
    setError(undefined);
    try {
      const settings = policy.supported
        .filter((setting) => valueOf(draft, setting.key))
        .map((setting) => ({ key: setting.key, value: valueOf(draft, setting.key) }));
      onSaved(await requirePolicySettings(operatorToken, settings));
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "The policy could not be saved.");
    } finally {
      setSaving(false);
    }
  }

  return (
    <>
      <p className="apiary-policy-note">
        A required setting is locked on every Hive in this Apiary. A Hive that can&apos;t accept it has to leave the Apiary.
        Credentials, file paths, what workers may run, and restarts are never policy.
      </p>
      <ul className="apiary-policy-list" aria-label="Apiary policy settings">
        {policy.supported.map((setting) => {
          const change = (value: string) => setDraft((current) => ({ ...current, [setting.key]: value }));
          if (setting.kind === "hours") {
            const [start = "", end = ""] = (draft[setting.key] ?? "").split("-");
            const value = valueOf(draft, setting.key);
            const wrong = value !== "" && !validHours(value);
            const hintId = `apiary-policy-hint-${setting.key}`;
            // Both empty is "each Hive decides"; one empty is not a window yet.
            const changeTime = (which: "start" | "end", time: string) => {
              const next = which === "start" ? [time, end] : [start, time];
              change(next[0] === "" && next[1] === "" ? "" : `${next[0]}-${next[1]}`);
            };
            return (
              <li key={setting.key}>
                <fieldset className="apiary-policy-hours" aria-describedby={hintId}>
                  <legend>{settingName(setting.key)}</legend>
                  <label><span>Starts</span><input type="time" value={start} disabled={saving} aria-invalid={wrong} onChange={(event) => changeTime("start", event.currentTarget.value)} /></label>
                  <label><span>Ends</span><input type="time" value={end} disabled={saving} aria-invalid={wrong} onChange={(event) => changeTime("end", event.currentTarget.value)} /></label>
                </fieldset>
                <small id={hintId} className={wrong ? "apiary-policy-hint is-wrong" : "apiary-policy-hint"}>
                  {wrong
                    ? "Choose both times, and make them different."
                    : "In each Hive's own time zone. Leave both empty and each Hive keeps its own hours."}
                </small>
              </li>
            );
          }
          if (setting.kind === "version") {
            const value = valueOf(draft, setting.key);
            const wrong = value !== "" && !RELEASE_NUMBER.test(value);
            const hintId = `apiary-policy-hint-${setting.key}`;
            return (
              <li key={setting.key}>
                <label>
                  <span>{settingName(setting.key)}</span>
                  <input
                    type="text"
                    inputMode="decimal"
                    autoComplete="off"
                    spellCheck={false}
                    placeholder="Each Hive decides"
                    value={draft[setting.key] ?? ""}
                    disabled={saving}
                    aria-invalid={wrong}
                    aria-describedby={hintId}
                    onChange={(event) => change(event.currentTarget.value)}
                  />
                </label>
                <small id={hintId} className={wrong ? "apiary-policy-hint is-wrong" : "apiary-policy-hint"}>
                  {wrong
                    ? "Use a release number, such as 1.16.9."
                    : "A Hive below it keeps its own work but can't take Apiary tasks until it updates. Leave empty and every release can."}
                </small>
              </li>
            );
          }
          return (
            <li key={setting.key}>
              <label>
                <span>{settingName(setting.key)}</span>
                <select value={draft[setting.key] ?? ""} disabled={saving} onChange={(event) => change(event.currentTarget.value)}>
                  <option value="">Each Hive decides</option>
                  {setting.values.map((value) => <option key={value} value={value}>{settingValue(setting.key, value)}</option>)}
                </select>
              </label>
            </li>
          );
        })}
      </ul>
      <footer className="apiary-policy-actions">
        {error ? <small role="alert">{error}</small> : <small>Hives pick up a change within a few seconds of saving.</small>}
        <button type="button" className="primary-action" disabled={!dirty || invalid || saving} onClick={() => void save()}>
          {saving ? "Saving…" : "Save policy"}
        </button>
      </footer>
    </>
  );
}

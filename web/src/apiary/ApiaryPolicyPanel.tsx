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
    return policy.supported.some((setting) => (draft[setting.key] ?? "") !== (saved[setting.key] ?? ""));
  }, [draft, policy]);

  async function save() {
    setSaving(true);
    setError(undefined);
    try {
      const settings = policy.supported
        .filter((setting) => draft[setting.key])
        .map((setting) => ({ key: setting.key, value: draft[setting.key] }));
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
        {policy.supported.map((setting) => (
          <li key={setting.key}>
            <label>
              <span>{settingName(setting.key)}</span>
              <select
                value={draft[setting.key] ?? ""}
                disabled={saving}
                onChange={(event) => { const value = event.currentTarget.value; setDraft((current) => ({ ...current, [setting.key]: value })); }}
              >
                <option value="">Each Hive decides</option>
                {setting.values.map((value) => <option key={value} value={value}>{settingValue(setting.key, value)}</option>)}
              </select>
            </label>
          </li>
        ))}
      </ul>
      <footer className="apiary-policy-actions">
        {error ? <small role="alert">{error}</small> : <small>Hives pick up a change within a few seconds of saving.</small>}
        <button type="button" className="primary-action" disabled={!dirty || saving} onClick={() => void save()}>
          {saving ? "Saving…" : "Save policy"}
        </button>
      </footer>
    </>
  );
}

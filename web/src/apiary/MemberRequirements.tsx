import type { ApiaryPolicySetting } from "../api";
import { settingName, settingValue } from "./policyLabels";

/**
 * What this Hive's Apiary requires of it (ADR 0113), on its own Apiary page.
 *
 * ⚠️ SAID WHERE THE OPERATOR LIVES, NOT ONLY WHERE A SETTING IS LOCKED. A
 * required setting overrides this Hive's own choice, and finding that out from
 * a greyed-out checkbox is finding it out by accident. Read with the rest of
 * the page so it recovers when the page does, and rendered only when the Apiary
 * requires something: an empty panel would be noise.
 */
export default function MemberRequirements({ settings }: { settings: ApiaryPolicySetting[] }) {
  if (settings.length === 0) return null;
  return (
    <article className="keeper-panel member-requirements" aria-labelledby="member-requirements-heading">
      <header><div><p className="eyebrow">Policy</p><h4 id="member-requirements-heading">Your Apiary requires</h4></div></header>
      <ul className="member-requirements-list">
        {settings.map((setting) => (
          <li key={setting.key}><span>{settingName(setting.key)}</span><strong>{settingValue(setting.key, setting.value)}</strong></li>
        ))}
      </ul>
      <p className="member-sync-copy">
        Set by your Apiary&apos;s Keeper, so they can&apos;t be changed on this Hive. If this Hive can&apos;t run with them, leaving the Apiary is the way out.
      </p>
    </article>
  );
}

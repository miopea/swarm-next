import { useEffect, useImperativeHandle, useState, type Ref } from "react";
import { fetchPublicHiveProfile, saveJoinPublicProfile, savePublicHiveProfile, type PublicHiveProfile } from "../api";

export type JoinPublicProfileHandle = { save: () => Promise<void> };

/** This preview does not publish anything. The enclosing join action saves it. */
export default function JoinPublicProfile({ operatorToken, disabled, ref, joining = true }: {
  operatorToken: string; disabled: boolean; ref: Ref<JoinPublicProfileHandle>; joining?: boolean;
}) {
  const [profile, setProfile] = useState<PublicHiveProfile>();
  const [failed, setFailed] = useState(false);
  const [attempt, setAttempt] = useState(0);
  useEffect(() => {
    const controller = new AbortController();
    setProfile(undefined);
    setFailed(false);
    void fetchPublicHiveProfile(operatorToken, controller.signal).then((result) => {
      if (!controller.signal.aborted) setProfile(result.profile);
    }).catch(() => { if (!controller.signal.aborted) setFailed(true); });
    return () => controller.abort();
  }, [operatorToken, attempt]);

  useImperativeHandle(ref, () => ({ save: async () => {
    if (!profile) throw new Error("Your shared profile has not loaded. Retry it before continuing.");
    if (!profile.operator_display_name.trim() || profile.operator_display_name.trim() === "Operator") {
      throw new Error("Enter your name so the Apiary can recognize you.");
    }
    const saved = await (joining ? saveJoinPublicProfile : savePublicHiveProfile)(operatorToken, {
      hive_name: profile.hive_name.trim(), operator_display_name: profile.operator_display_name.trim(),
      contact_email: profile.contact_email?.trim() || null,
    });
    setProfile(saved.profile);
  } }), [operatorToken, profile, joining]);

  const firstName = profile?.operator_display_name.trim().split(/\s+/)[0];
  const proposedName = joining && profile?.hive_name.trim() === "My Hive" && firstName && firstName !== "Operator"
    ? `${firstName}'s Hive` : profile?.hive_name;
  return <section className="apiary-join-card" aria-label="Your shared Apiary profile">
    <div><strong>How the Apiary will see you</strong><small>Your name, Hive name, and optional contact email are shared with its members. Private work and credentials stay here.</small></div>
    {failed ? <p role="alert">Your profile could not be loaded. <button type="button" className="secondary-button" onClick={() => setAttempt((value) => value + 1)}>Retry profile</button></p>
      : !profile ? <p role="status">Loading your profile…</p> : <>
        <fieldset disabled={disabled} className="apiary-public-profile-fields">
          <label>Your name<input value={profile.operator_display_name === "Operator" ? "" : profile.operator_display_name} autoComplete="name" maxLength={120} onChange={(event) => setProfile({ ...profile, operator_display_name: event.target.value })} /></label>
          <label>Hive name<input value={profile.hive_name} maxLength={120} onChange={(event) => setProfile({ ...profile, hive_name: event.target.value })} /></label>
          <label>Contact email (optional)<input type="email" autoComplete="email" value={profile.contact_email ?? ""} maxLength={254} onChange={(event) => setProfile({ ...profile, contact_email: event.target.value })} /></label>
        </fieldset>
        <p aria-label="Shared profile preview"><strong>{proposedName}</strong> · {profile.operator_display_name === "Operator" ? "Your name" : profile.operator_display_name}{profile.contact_email ? ` · ${profile.contact_email}` : " · No contact email"}</p>
        <small>{joining ? "Saved when you connect or join. Only the default “My Hive” becomes your first name’s Hive; a custom name stays unchanged." : "Save updates this Hive first; the Apiary receives the changes through its normal synchronization. Membership and private work do not change."} Email is contact information, not a verified login.</small>
      </>}
  </section>;
}

import { useEffect, useImperativeHandle, useRef, useState, type Ref } from "react";
import { fetchPublicHiveProfile, saveJoinPublicProfile, savePublicHiveProfile, type PublicHiveProfile } from "../api";
import { connectedIdentities, type ConnectedIdentity } from "../shared/connectedIdentity";

export type JoinPublicProfileHandle = { save: () => Promise<void> };

/** This preview does not publish anything. The enclosing setup action saves it. */
export default function JoinPublicProfile({ operatorToken, disabled, ref, joining = true }: {
  operatorToken: string; disabled: boolean; ref: Ref<JoinPublicProfileHandle>; joining?: boolean;
}) {
  const [profile, setProfile] = useState<PublicHiveProfile>();
  const [failed, setFailed] = useState(false);
  const [attempt, setAttempt] = useState(0);
  const [identities, setIdentities] = useState<ConnectedIdentity[]>([]);
  const edited = useRef({ name: false, email: false });
  useEffect(() => {
    const controller = new AbortController();
    let loaded = false;
    const deadline = window.setTimeout(() => { controller.abort(); if (!loaded) setFailed(true); }, 8_000);
    edited.current = { name: false, email: false };
    setIdentities([]);
    setProfile(undefined);
    setFailed(false);
    void fetchPublicHiveProfile(operatorToken, controller.signal).then(async (result) => {
      if (controller.signal.aborted) return;
      loaded = true;
      setProfile(result.profile);
      const current = result.profile;
      if (current.operator_display_name.trim() !== "Operator" && current.operator_display_name.trim() && current.contact_email?.trim()) return;
      const candidates = await connectedIdentities(operatorToken, controller.signal);
      if (controller.signal.aborted) return;
      setIdentities(candidates);
      // Two distinct accounts require a choice. Do not assemble one identity
      // from one account's name and a different account's email.
      if (candidates.length === 1) {
        const candidate = candidates[0];
        setProfile((saved) => {
          if (!saved) return saved;
          if (saved.contact_email && saved.contact_email !== candidate.email) return saved;
          const missingName = !saved.operator_display_name.trim() || saved.operator_display_name.trim() === "Operator";
          if (!missingName && saved.operator_display_name.trim() !== candidate.name) return saved;
          return { ...saved,
            operator_display_name: missingName && !edited.current.name && candidate.name ? candidate.name : saved.operator_display_name,
            contact_email: !saved.contact_email && !edited.current.email && candidate.email ? candidate.email : saved.contact_email,
          };
        });
      }
    }).catch(() => { if (!controller.signal.aborted && !loaded) setFailed(true); })
      .finally(() => window.clearTimeout(deadline));
    return () => { controller.abort(); window.clearTimeout(deadline); };
  }, [operatorToken, attempt]);

  useImperativeHandle(ref, () => ({ save: async () => {
    if (!profile) throw new Error("Your shared profile has not loaded. Retry it before continuing.");
    if (!profile.operator_display_name.trim() || profile.operator_display_name.trim() === "Operator") {
      throw new Error("Enter your name so the Apiary can recognize you.");
    }
    // Once the reviewed snapshot is submitted, late account lookups cannot
    // change what the form shows relative to what was actually saved.
    edited.current = { name: true, email: true };
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
        {identities.length > 0 && <div aria-label="Connected account identity">
          <small>{identities.length > 1 ? "Choose which connected account represents you, or keep your saved details." : "Available from your connected account. Review the shared details below."}</small>
          {identities.map((identity) => <button type="button" className="secondary-button" disabled={disabled} key={identity.source} onClick={() => {
            edited.current = { name: true, email: true };
            setProfile({ ...profile, operator_display_name: identity.name || profile.operator_display_name, contact_email: identity.email || profile.contact_email });
          }}>Use {identity.source}: {identity.name}{identity.email ? ` · ${identity.email}` : ""}</button>)}
        </div>}
        <fieldset disabled={disabled} className="apiary-public-profile-fields">
          <label>Your name<input value={profile.operator_display_name === "Operator" ? "" : profile.operator_display_name} autoComplete="name" maxLength={120} onChange={(event) => { edited.current.name = true; setProfile({ ...profile, operator_display_name: event.target.value }); }} /></label>
          <label>Hive name<input value={profile.hive_name} maxLength={120} onChange={(event) => setProfile({ ...profile, hive_name: event.target.value })} /></label>
          <label>Contact email (optional)<input type="email" autoComplete="email" value={profile.contact_email ?? ""} maxLength={254} onChange={(event) => { edited.current.email = true; setProfile({ ...profile, contact_email: event.target.value }); }} /></label>
        </fieldset>
        <p aria-label="Shared profile preview"><strong>{proposedName}</strong> · {profile.operator_display_name === "Operator" ? "Your name" : profile.operator_display_name}{profile.contact_email ? ` · ${profile.contact_email}` : " · No contact email"}</p>
        <small>{joining ? "Saved when you connect, join, or create an Apiary. Only the default “My Hive” becomes your first name’s Hive; a custom name stays unchanged." : "Save updates this Hive first; the Apiary receives the changes through its normal synchronization. Membership and private work do not change."} Email is contact information, not a verified login.</small>
      </>}
  </section>;
}

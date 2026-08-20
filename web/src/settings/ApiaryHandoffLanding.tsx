import { useState, type FormEvent, type ReactNode } from "react";

import BeeMascot from "../brand/BeeMascot";
import {
  currentApiaryHandoffLink, retargetApiaryHandoffLink, stageApiaryHandoff,
} from "./apiaryHandoff";

type Props = { children: ReactNode };

export default function ApiaryHandoffLanding({ children }: Props) {
  const [link, setLink] = useState(() => currentApiaryHandoffLink("keeper"));
  const [hiveAddress, setHiveAddress] = useState("");
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");

  if (!link) return children;

  function useThisHive() {
    if (!link) return;
    try {
      stageApiaryHandoff("keeper", link);
      window.history.replaceState(null, "", "/#settings-apiary");
      setLink(undefined);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "That private invitation link could not be read.");
    }
  }

  function openPersonalHive(event: FormEvent) {
    event.preventDefault();
    if (!link) return;
    setError("");
    try {
      window.location.replace(retargetApiaryHandoffLink(link, hiveAddress, "keeper"));
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "That personal Hive address could not be opened.");
    }
  }

  async function copyLink() {
    if (!link) return;
    setError("");
    try {
      await navigator.clipboard.writeText(link);
      setMessage("Private link copied. Paste it into Settings → Apiary in your personal Hive.");
    } catch {
      setError("The browser could not copy the link. Copy the complete address from the address bar instead.");
    }
  }

  return (
    <main className="apiary-handoff-page">
      <section className="apiary-handoff-card" aria-labelledby="apiary-handoff-heading">
        <div className="apiary-handoff-bee"><BeeMascot role="queen" expression="available" /></div>
        <p className="eyebrow">Private Apiary invitation</p>
        <h1 id="apiary-handoff-heading">Open this in your personal Hive</h1>
        <p className="apiary-handoff-lead">Your Hive connects outward to the Keeper. The private invitation stays in this browser&apos;s URL fragment and is never sent to a handoff service.</p>
        <form className="apiary-handoff-target" onSubmit={openPersonalHive}>
          <label htmlFor="personal-hive-address">Your personal Hive address</label>
          <div><input id="personal-hive-address" type="url" inputMode="url" autoCapitalize="none" autoCorrect="off" placeholder="https://my-hive.example.com" value={hiveAddress} onChange={(event) => setHiveAddress(event.target.value)} /><button disabled={!hiveAddress.trim()}>Open my Hive</button></div>
          <small>Use the address where you normally open Swarm. HTTPS is required unless it runs on localhost.</small>
        </form>
        <div className="apiary-handoff-current">
          <span><strong>Already opened in your personal Hive?</strong><small>Continue here, then review the exact Keeper and Apiary before joining.</small></span>
          <button className="secondary-button" type="button" onClick={useThisHive}>Use this Hive</button>
        </div>
        <button className="apiary-handoff-copy" type="button" onClick={() => void copyLink()}>Copy link for manual paste</button>
        {message ? <p className="form-message" role="status">{message}</p> : null}
        {error ? <p className="form-error" role="alert">{error}</p> : null}
        <p className="apiary-handoff-safety"><strong>No membership happens yet.</strong> Your Hive first introduces only its signed public identity. The Keeper approves that exact Hive; policy, Jira readiness, and final joining remain explicit.</p>
      </section>
    </main>
  );
}

import { useEffect, useState } from "react";

import { beginEmailAuthorization, disconnectEmail, fetchEmailConfiguration, type EmailOAuthConfiguration, type EmailReadiness } from "../api";

type Props = {
  operatorToken: string;
  readiness: EmailReadiness | undefined;
  unavailable: boolean;
  onRetryReadiness?: () => void;
  onNavigate?: (url: string) => void;
};

export default function EmailSettings({ operatorToken, readiness, unavailable, onRetryReadiness, onNavigate = (url) => window.location.assign(url) }: Props) {
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  const [configuration, setConfiguration] = useState<EmailOAuthConfiguration>();

  useEffect(() => {
    let current = true;
    void fetchEmailConfiguration(operatorToken)
      .then((next) => {
        if (!current) return;
        setConfiguration(next);
      })
      .catch((error: unknown) => {
        if (current) setMessage(error instanceof Error ? error.message : "Microsoft app setup could not be loaded.");
      });
    return () => { current = false; };
  }, [operatorToken]);

  async function connect() {
    setBusy(true);
    setMessage("");
    try {
      onNavigate(await beginEmailAuthorization(operatorToken));
    } catch (error) {
      setMessage(error instanceof Error ? error.message : "Microsoft authorization could not start.");
      setBusy(false);
    }
  }

  async function disconnect() {
    setBusy(true);
    setMessage("");
    try {
      await disconnectEmail(operatorToken);
      window.location.reload();
    } catch (error) {
      setMessage(error instanceof Error ? error.message : "Outlook could not be disconnected.");
      setBusy(false);
    }
  }

  // Swarm ships with a registered Microsoft application, so a fresh Hive is
  // already configured and the only thing left is consent.
  const connected = readiness?.connection === "ready";
  return (
    <section id="settings-email" className="settings-card integration-settings email-settings" aria-labelledby="email-integration-heading">
      <div><p className="eyebrow">Email intake</p><h3 id="email-integration-heading">Turn reported issues into finished work</h3></div>
      <p>Link one Microsoft Outlook account. Choose messages from Inbox on the task board; Swarm preserves the readable message, images, attachments, and original thread.</p>
      <div className="integration-status" role="status">
        <span className={`presence ${connected ? "online" : unavailable || readiness?.connection === "credentials_invalid" || readiness?.connection === "permission_denied" ? "offline" : "waiting"}`} />
        <span><strong>{readinessLabel(readiness, unavailable)}</strong><small>{readinessDetail(readiness, unavailable)}</small></span>
        {unavailable && onRetryReadiness ? <button className="secondary-button" type="button" onClick={onRetryReadiness}>Retry Outlook status</button> : null}
      </div>
      {connected ? (
        <button className="secondary-button jira-auth-action" type="button" disabled={busy} onClick={() => void disconnect()}>Disconnect Outlook</button>
      ) : unavailable ? null : (
        <div className="jira-connect-panel">
          <button className="primary-action jira-auth-action" type="button" disabled={busy} onClick={() => void connect()}>
            {busy ? "Opening Microsoft…" : readiness?.connection === "credentials_invalid" ? "Reconnect Outlook" : "Connect Outlook"}
          </button>
          {/* THE CALLBACK STAYS ON SCREEN EVEN THOUGH NOBODY TYPES IT. It is
              the one value whoever maintains the shared registration needs:
              Microsoft matches redirect URIs exactly, so a Hive published at
              its own address is refused with AADSTS50011 until this exact
              string is on the registration. A Hive left on localhost is
              already covered, because the port is ignored for loopback
              matching. */}
          <label className="email-callback-field">
            This Hive's redirect URI
            <input readOnly value={configuration?.callback_url ?? "This Hive does not know its own address yet"} onFocus={(event) => event.currentTarget.select()} />
          </label>
          <small className="privacy-note">
            Personal and work accounts both work — sign in with the address you want Swarm to read, and Microsoft routes it. Nothing to register and no secret to store.
            A Microsoft consent page opens, then returns here. Mail tokens remain private on this host and never enter Queen, workers, or browser storage.
          </small>
        </div>
      )}
      <div className="integration-guardrails">
        <strong>Closed-loop by design</strong>
        <span>Import is always your choice. Completing a task does not send mail. A readable resolution reply becomes available only after completion and recorded deployment, and you review it before sending.</span>
      </div>
      {message ? <p className="settings-message" role="status">{message}</p> : null}
    </section>
  );
}

function readinessLabel(readiness: EmailReadiness | undefined, unavailable: boolean) {
  if (unavailable) return "Outlook status unavailable";
  switch (readiness?.connection) {
    case "ready": return readiness.account_address ? `Connected as ${readiness.account_address}` : "Outlook connected";
    case "credentials_invalid": return "Outlook authorization needs attention";
    case "permission_denied": return "Mailbox access was denied";
    case "network_unavailable": return "Outlook is temporarily unavailable";
    case "not_connected": return "Outlook not connected";
    default: return "Checking Outlook";
  }
}

function readinessDetail(readiness: EmailReadiness | undefined, unavailable: boolean) {
  if (unavailable) return "Local workers and tasks remain available.";
  switch (readiness?.connection) {
    case "ready": return readiness.account_name ? `Inbox access uses ${readiness.account_name}'s delegated identity.` : "Inbox access uses your delegated Microsoft identity.";
    case "credentials_invalid": return "Reconnect the account to continue importing or replying.";
    case "permission_denied": return "Swarm needs delegated Mail.Read and Mail.Send access for this workflow.";
    case "network_unavailable": return "Imported tasks remain available; Inbox and replies wait.";
    case "not_connected": return readiness?.configured ? "Connect the one account this Hive uses for issue intake." : "This host needs its one-time Microsoft app configuration.";
    default: return "No mailbox content is read until you open Email work.";
  }
}

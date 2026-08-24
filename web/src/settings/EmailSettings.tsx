import { useEffect, useState, type FormEvent } from "react";

import { beginEmailAuthorization, disconnectEmail, fetchEmailConfiguration, updateEmailConfiguration, type EmailOAuthConfiguration, type EmailReadiness } from "../api";

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
  const [editingConfiguration, setEditingConfiguration] = useState(false);
  const [tenantId, setTenantId] = useState("");
  const [clientId, setClientId] = useState("");
  const [clientSecret, setClientSecret] = useState("");

  useEffect(() => {
    let current = true;
    void fetchEmailConfiguration(operatorToken)
      .then((next) => {
        if (!current) return;
        setConfiguration(next);
        setTenantId(next.tenant_id ?? "");
        setClientId(next.client_id ?? "");
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

  async function saveConfiguration(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setBusy(true);
    setMessage("");
    try {
      const next = await updateEmailConfiguration(operatorToken, tenantId, clientId, clientSecret);
      setConfiguration(next);
      setClientSecret("");
      setEditingConfiguration(false);
      setMessage("Microsoft app registration saved privately. You can connect Outlook now.");
    } catch (error) {
      setMessage(error instanceof Error ? error.message : "Microsoft app setup could not be saved.");
    } finally {
      setBusy(false);
    }
  }

  const connected = readiness?.connection === "ready";
  const configured = readiness?.configured === true || configuration?.configured === true;
  const canManageConfiguration = configuration?.managed_by !== "environment";
  const showConfigurationForm = !unavailable && configuration !== undefined && !connected && canManageConfiguration && (!configured || editingConfiguration);
  return (
    <section id="settings-email" className="settings-card integration-settings email-settings" aria-labelledby="email-integration-heading">
      <div><p className="eyebrow">Email intake</p><h3 id="email-integration-heading">Turn reported issues into finished work</h3></div>
      <p>Link one Microsoft Outlook account. Choose messages from Inbox on the task board; Swarm preserves the readable message, images, attachments, and original thread.</p>
      <div className="integration-status" role="status">
        <span className={`presence ${connected ? "online" : unavailable || readiness?.connection === "credentials_invalid" || readiness?.connection === "permission_denied" ? "offline" : "waiting"}`} />
        <span><strong>{readinessLabel(readiness, unavailable)}</strong><small>{readinessDetail(readiness, unavailable)}</small></span>
        {unavailable && onRetryReadiness ? <button className="secondary-button" type="button" onClick={onRetryReadiness}>Retry Outlook status</button> : null}
      </div>
      {showConfigurationForm ? (
        <form className="email-configuration" aria-label="Microsoft app setup" onSubmit={(event) => void saveConfiguration(event)}>
          <div className="email-configuration-heading">
            <div><strong>One-time Microsoft app setup</strong><small>Register a Web application in Microsoft Entra, then enter its three values here.</small></div>
            <a className="secondary-button compact-action" href="https://entra.microsoft.com/#view/Microsoft_AAD_RegisteredApps/ApplicationsListBlade" target="_blank" rel="noreferrer">Open Entra</a>
          </div>
          <ol className="email-setup-steps">
            <li>Add this exact <strong>Web redirect URI</strong>.</li>
            <li>Add delegated permissions: <strong>User.Read, Mail.Read, Mail.Send</strong>.</li>
            <li>Create a client secret, then save its value here before leaving Entra.</li>
          </ol>
          <label className="email-callback-field">Web redirect URI<input readOnly value={configuration?.callback_url ?? "This Hive does not know its own address yet"} onFocus={(event) => event.currentTarget.select()} /></label>
          <div className="email-configuration-fields">
            <label>Directory (tenant) ID<input required autoComplete="off" value={tenantId} onChange={(event) => setTenantId(event.target.value)} placeholder="organizations or tenant UUID" /></label>
            <label>Application (client) ID<input required autoComplete="off" value={clientId} onChange={(event) => setClientId(event.target.value)} placeholder="00000000-0000-0000-0000-000000000000" /></label>
            <label>Client secret value<input required type="password" autoComplete="new-password" value={clientSecret} onChange={(event) => setClientSecret(event.target.value)} placeholder="Shown once by Microsoft" /></label>
          </div>
          <small className="privacy-note">The secret travels only to this Hive over HTTPS, is stored in its private host directory, and is never returned to the browser, Queen, or workers.</small>
          <div className="email-configuration-actions">
            {configured ? <button className="secondary-button" type="button" disabled={busy} onClick={() => setEditingConfiguration(false)}>Cancel</button> : null}
            <button className="primary-action" type="submit" disabled={busy}>{busy ? "Saving privately…" : "Save app registration"}</button>
          </div>
        </form>
      ) : connected ? (
        <button className="secondary-button jira-auth-action" type="button" disabled={busy} onClick={() => void disconnect()}>Disconnect Outlook</button>
      ) : unavailable ? null : (
        <div className="jira-connect-panel">
          <div className="email-connect-actions">
            <button className="primary-action jira-auth-action" type="button" disabled={busy || unavailable || !configured} onClick={() => void connect()}>
              {busy ? "Opening Microsoft…" : readiness?.connection === "credentials_invalid" ? "Reconnect Outlook" : "Connect Outlook"}
            </button>
            {canManageConfiguration && configured ? <button className="secondary-button" type="button" disabled={busy} onClick={() => setEditingConfiguration(true)}>Replace app registration</button> : null}
          </div>
          <small className="privacy-note">A Microsoft consent page opens, then returns here. Mail tokens remain private on this host and never enter Queen, workers, or browser storage.</small>
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

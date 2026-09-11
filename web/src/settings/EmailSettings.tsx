import { useEffect, useState, type FormEvent } from "react";

import { beginEmailAuthorization, disconnectEmail, fetchEmailConfiguration, updateEmailConfiguration, type EmailOAuthConfiguration, type EmailReadiness } from "../api";

/// `consumers` is personal Microsoft accounts only, and `organizations` is work
/// and school only. Neither accepts the other, which is the point: a mailbox
/// belongs to one of them, and a precise authority gives a refusal that names
/// the real problem instead of a sign-in page that just says no.
///
/// ⚠️ `organizations` SILENTLY EXCLUDES EVERY PERSONAL ACCOUNT. Defaulting to it
/// -- which this screen used to do -- is invisible until someone with an
/// outlook.com address tries to connect.
const PERSONAL_AUTHORITY = "consumers";
const WORK_AUTHORITY = "organizations";

type AccountTypeId = "personal" | "work";

const ACCOUNT_TYPES: { id: AccountTypeId; authority: string; label: string; detail: string; entraChoice: string }[] = [
  {
    id: "personal",
    authority: PERSONAL_AUTHORITY,
    label: "Personal Microsoft account",
    detail: "outlook.com, hotmail.com, live.com, or any address you signed up with yourself.",
    entraChoice: "Personal Microsoft accounts only",
  },
  {
    id: "work",
    authority: WORK_AUTHORITY,
    label: "Work or school (Microsoft 365)",
    detail: "An address your organisation issued you. Sign-in follows your organisation's policy.",
    entraChoice: "Accounts in any organizational directory",
  },
];

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
  const [tenantId, setTenantId] = useState(PERSONAL_AUTHORITY);
  const [clientId, setClientId] = useState("");
  const [clientSecret, setClientSecret] = useState("");
  const [showAdvanced, setShowAdvanced] = useState(false);

  useEffect(() => {
    let current = true;
    void fetchEmailConfiguration(operatorToken)
      .then((next) => {
        if (!current) return;
        setConfiguration(next);
        setTenantId(next.tenant_id ?? PERSONAL_AUTHORITY);
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
      setShowAdvanced(false);
      setEditingConfiguration(false);
      setMessage("Microsoft app registration saved privately. You can connect Outlook now.");
    } catch (error) {
      setMessage(error instanceof Error ? error.message : "Microsoft app setup could not be saved.");
    } finally {
      setBusy(false);
    }
  }

  // WORK OR SCHOOL IS THE FALLBACK, not personal. A saved tenant UUID is a
  // single-tenant corporate registration, and there are far more possible UUIDs
  // than there are keywords -- so the unknown case has to land on "work".
  const accountType: AccountTypeId = tenantId === PERSONAL_AUTHORITY || tenantId === "common" ? "personal" : "work";
  // Swarm ships with a registered Microsoft application, so a fresh Hive is
  // already configured and the only thing left is consent.
  const bundled = configuration?.managed_by === "bundled";
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
            <div><strong>One-time Microsoft app setup</strong><small>Register a Mobile and desktop application in Microsoft Entra, then paste its Application ID. No client secret is needed.</small></div>
            <a className="secondary-button compact-action" href="https://entra.microsoft.com/#view/Microsoft_AAD_RegisteredApps/ApplicationsListBlade" target="_blank" rel="noreferrer">Open Entra</a>
          </div>
          <fieldset className="email-account-type">
            <legend>Which kind of Microsoft account is this mailbox?</legend>
            {ACCOUNT_TYPES.map((option) => (
              <label key={option.authority}>
                <input
                  type="radio"
                  name="email-account-type"
                  value={option.authority}
                  checked={accountType === option.id}
                  onChange={() => setTenantId(option.authority)}
                />
                <span><strong>{option.label}</strong><small>{option.detail}</small></span>
              </label>
            ))}
          </fieldset>
          <ol className="email-setup-steps">
            <li>New registration. For <strong>Supported account types</strong>, choose <strong>{ACCOUNT_TYPES.find((option) => option.id === accountType)?.entraChoice}</strong>.</li>
            <li>Under <strong>Mobile and desktop applications</strong>, add this exact redirect URI. Not <em>Web</em>, and not <em>Single-page application</em> — an SPA redirect expires its refresh token every 24 hours and this Hive runs unattended.</li>
            <li>API permissions, delegated: <strong>User.Read, Mail.Read, Mail.Send</strong>. All three are consentable by the person signing in; none needs an administrator.</li>
          </ol>
          <label className="email-callback-field">Redirect URI<input readOnly value={configuration?.callback_url ?? "This Hive does not know its own address yet"} onFocus={(event) => event.currentTarget.select()} /></label>
          <div className="email-configuration-fields">
            <label>Application (client) ID<input required autoComplete="off" value={clientId} onChange={(event) => setClientId(event.target.value)} placeholder="00000000-0000-0000-0000-000000000000" /></label>
          </div>
          <button className="link-button" type="button" aria-expanded={showAdvanced} onClick={() => setShowAdvanced((open) => !open)}>
            {showAdvanced ? "Hide advanced settings" : "Advanced settings"}
          </button>
          {showAdvanced ? (
            <div className="email-configuration-fields">
              <label>Directory (tenant) ID<input autoComplete="off" value={tenantId} onChange={(event) => setTenantId(event.target.value)} placeholder="common, consumers, organizations, or a tenant UUID" /></label>
              <label>Client secret value (not needed)<input type="password" autoComplete="new-password" value={clientSecret} onChange={(event) => setClientSecret(event.target.value)} placeholder="Leave empty" /></label>
            </div>
          ) : null}
          <small className="privacy-note">Swarm signs in as a public client using PKCE, so there is no secret to store or rotate. A secret is only for a Hive that already registered a confidential Web application; anything entered above travels only to this Hive over HTTPS and is never returned to the browser, Queen, or workers.</small>
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
            {canManageConfiguration && configured ? <button className="secondary-button" type="button" disabled={busy} onClick={() => setEditingConfiguration(true)}>{bundled ? "Use your own Microsoft app" : "Replace app registration"}</button> : null}
          </div>
          {bundled ? (
            // THE CALLBACK STAYS VISIBLE EVEN WHEN THERE IS NOTHING TO FILL IN.
            // Hiding the setup form hid this with it, and it is the one value
            // whoever maintains the shared registration needs: Microsoft
            // matches redirect URIs exactly, so a Hive published at its own
            // address is refused with AADSTS50011 until this exact string is
            // on the registration. A Hive left on localhost is already covered
            // -- the port is ignored for loopback matching.
            <label className="email-callback-field">
              This Hive's redirect URI
              <input readOnly value={configuration?.callback_url ?? "This Hive does not know its own address yet"} onFocus={(event) => event.currentTarget.select()} />
            </label>
          ) : null}
          <small className="privacy-note">
            {bundled ? "Personal and work accounts both work — sign in with the address you want Swarm to read, and Microsoft routes it. Nothing to register and no secret to store. " : null}
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

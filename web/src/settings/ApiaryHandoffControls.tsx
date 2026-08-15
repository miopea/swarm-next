import { useId, useState, type ReactNode } from "react";

import type { FederationTransportReadiness } from "../api";

type ApiaryExchangeStepProps = {
  number: string;
  title: string;
  detail: string;
  children?: ReactNode;
};

export function ApiaryExchangeStep({ number, title, detail, children }: ApiaryExchangeStepProps) {
  return (
    <li>
      <span className="apiary-step-number" aria-hidden="true">{number}</span>
      <span className="apiary-step-copy"><strong>{title}</strong><small>{detail}</small></span>
      {children ? <span className="apiary-step-action">{children}</span> : null}
    </li>
  );
}

export function ApiaryGeneratedLink({ link, onCopy }: { link: string; onCopy: (link: string) => Promise<boolean> }) {
  const id = useId();
  return (
    <div className="apiary-generated-link" role="group" aria-label="Created Apiary link">
      <label htmlFor={id}><span>Private handoff link</span><input id={id} readOnly value={link} onFocus={(event) => event.currentTarget.select()} /></label>
      <button className="secondary-button" onClick={() => void onCopy(link)}>Copy again</button>
      <small>Share only with the intended operator. The signed payload expires; invitation secrets are bound to one Hive and consumed once.</small>
    </div>
  );
}

type ApiaryLinkEntryProps = {
  label: string;
  value: string;
  action: string;
  disabled: boolean;
  onChange: (value: string) => void;
  onAction: () => void;
};

export function ApiaryLinkEntry({ label, value, action, disabled, onChange, onAction }: ApiaryLinkEntryProps) {
  const id = useId();
  return (
    <div className="apiary-link-entry">
      <label htmlFor={id}><span>{label}</span><input id={id} type="url" value={value} placeholder="Paste the complete link" onChange={(event) => onChange(event.target.value)} /></label>
      <button className="primary-action" disabled={disabled || !value.trim()} onClick={onAction}>{action}</button>
    </div>
  );
}

type ApiaryFileDropProps = {
  ariaLabel: string;
  disabled: boolean;
  label: string;
  detail: string;
  onFile: (file: File | undefined) => void;
};

export function ApiaryFileFallback(props: ApiaryFileDropProps & { summary: string }) {
  return <details className="apiary-file-fallback"><summary>{props.summary}</summary><ApiaryFileDrop {...props} /></details>;
}

function ApiaryFileDrop({ ariaLabel, disabled, label, detail, onFile }: ApiaryFileDropProps) {
  const [dragging, setDragging] = useState(false);
  const detailId = useId();
  return (
    <label
      className={`apiary-card-drop${dragging ? " drag-active" : ""}`}
      onDragEnter={(event) => { event.preventDefault(); if (!disabled) setDragging(true); }}
      onDragOver={(event) => event.preventDefault()}
      onDragLeave={() => setDragging(false)}
      onDrop={(event) => {
        event.preventDefault();
        setDragging(false);
        if (!disabled) onFile(event.dataTransfer.files[0]);
      }}
    >
      <input
        aria-label={ariaLabel}
        aria-describedby={detailId}
        type="file"
        accept="application/json,.json"
        disabled={disabled}
        onChange={(event) => { onFile(event.target.files?.[0]); event.currentTarget.value = ""; }}
      />
      <span>{label}</span>
      <small id={detailId}>{detail}</small>
    </label>
  );
}

export function FederationTransportStatus({ readiness }: { readiness: FederationTransportReadiness | undefined }) {
  const presentation = !readiness
    ? {
        title: "Checking this Hive's network address",
        detail: "Swarm is verifying whether another Hive can reach this installation.",
        tone: "waiting",
      }
    : readiness.reachability === "remote_https"
      ? {
          title: "Reachable Hive URL ready",
          detail: "Other Hives can contact this installation at the signed HTTPS address below. If this Hive becomes Keeper, it must remain online for invitations and shared coordination.",
          tone: "online",
        }
      : readiness.reachability === "local_only"
        ? {
            title: "Local testing only",
            detail: "A localhost or loopback address reaches only this machine. Configure a reachable HTTPS URL before another computer joins this Apiary.",
            tone: "waiting",
          }
        : {
            title: "Reachable Hive URL required",
            detail: "Configure this installation's public HTTPS URL before exchanging Apiary invitations. A private-network hostname is fine when every Hive can resolve and reach it.",
            tone: "offline",
          };
  return (
    <div className="apiary-network-readiness" aria-label="Hive network readiness" aria-live="polite">
      <span className={`presence ${presentation.tone}`} />
      <span>
        <strong>{presentation.title}</strong>
        <small>{presentation.detail}</small>
        {readiness?.endpoint ? <code>{readiness.endpoint}</code> : null}
      </span>
    </div>
  );
}

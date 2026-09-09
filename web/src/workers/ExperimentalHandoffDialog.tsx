import { useRef, useState } from "react";
import { TEMPORARY_PROVIDERS, type ProviderCapabilities, type ProviderKind, type Worker } from "../api";
import { ExperimentalProviderControl, isExperimentalProvider } from "../settings/ExperimentalProviderControl";
import { useModalFocus } from "../shared/useModalFocus";
import ModalPortal from "../shared/ModalPortal";

export default function ExperimentalHandoffDialog({ worker, provider, providers, capabilitiesUnavailable, onConfirm, onClose }: {
  worker: Worker;
  provider: ProviderKind;
  providers: ProviderCapabilities;
  capabilitiesUnavailable: boolean;
  onConfirm: () => Promise<void>;
  onClose: () => void;
}) {
  const [acknowledged, setAcknowledged] = useState(false);
  const label = TEMPORARY_PROVIDERS.find(choice => choice.provider === provider)?.label ?? provider;
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");
  const pending = useRef(false);
  const available = !capabilitiesUnavailable && isExperimentalProvider(provider) ? providers.experimental?.[provider] : undefined;
  const close = () => { if (!pending.current) onClose(); };
  const dialog = useModalFocus<HTMLDivElement>(close);
  async function confirm() {
    if (pending.current || !acknowledged || available !== true) return;
    pending.current = true;
    setSaving(true);
    setError("");
    try {
      await onConfirm();
      onClose();
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : "The temporary worker could not be created. Your current worker is unchanged.");
    } finally {
      pending.current = false;
      setSaving(false);
    }
  }
  return <ModalPortal><div className="dialog-backdrop" role="presentation" onClick={close}>
    <div className="dialog" ref={dialog} tabIndex={-1} role="dialog" aria-modal="true" aria-labelledby="experimental-handoff-heading" onClick={event => event.stopPropagation()}>
      <p className="eyebrow">Experimental provider</p>
      <h3 id="experimental-handoff-heading">Try {label} alongside {worker.name}</h3>
      <p>This creates a temporary worker in the same repository. {worker.name} keeps its current provider and conversation.</p>
      <ExperimentalProviderControl enabled={acknowledged} onChange={setAcknowledged} />
      {available !== true && <p role="status">{available === false ? "This provider is not available on the worker engine." : "The worker engine has not confirmed this provider is available."} No alternate provider will be selected.</p>}
      {error && <p role="alert">{error}</p>}
      <div className="dialog-actions">
        <button type="button" className="secondary-button" disabled={saving} onClick={close}>Cancel</button>
        <button type="button" disabled={saving || !acknowledged || available !== true} onClick={() => void confirm()}>{saving ? "Creating…" : "Create temporary worker"}</button>
      </div>
    </div>
  </div></ModalPortal>;
}

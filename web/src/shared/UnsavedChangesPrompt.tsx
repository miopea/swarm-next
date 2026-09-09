import { useRef } from "react";
import { useModalFocus } from "./useModalFocus";

export default function UnsavedChangesPrompt({
  label,
  description,
  discardLabel = "Discard changes",
  onDiscard,
  onKeep,
}: {
  label: string;
  description: string;
  discardLabel?: string;
  onDiscard: () => void;
  onKeep: () => void;
}) {
  const keep = useRef<HTMLButtonElement>(null);
  const dialog = useModalFocus<HTMLDivElement>(onKeep, true, keep);
  return <div ref={dialog} tabIndex={-1} className="modal-close-confirm" role="alertdialog" aria-modal="true" aria-label={label}>
    <p><strong>{label}</strong><span>{description}</span></p>
    <button type="button" className="danger-button" onClick={onDiscard}>{discardLabel}</button>
    <button ref={keep} type="button" onClick={onKeep}>Keep editing</button>
  </div>;
}

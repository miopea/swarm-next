import { useEffect, useRef } from "react";

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
  useEffect(() => { keep.current?.focus(); }, []);
  return <div className="modal-close-confirm" role="alertdialog" aria-label={label}>
    <p><strong>{label}</strong><span>{description}</span></p>
    <button type="button" className="danger-button" onClick={onDiscard}>{discardLabel}</button>
    <button ref={keep} type="button" onClick={onKeep}>Keep editing</button>
  </div>;
}

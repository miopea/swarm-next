import { useEffect, useRef, useState } from "react";

import type { Worker } from "../api";
import BeeMascot from "../brand/BeeMascot";
import { workerAttention } from "./workerAttention";

type Props = {
  worker: Worker;
  selected: boolean;
  detail: string;
  busy: boolean;
  onOpen: () => void;
  onStart: () => void;
  onStop: () => void;
};

export default function WorkerRosterItem({ worker, selected, detail, busy, onOpen, onStart, onStop }: Props) {
  const [menuOpen, setMenuOpen] = useState(false);
  const [, refreshAttention] = useState(0);
  const rowRef = useRef<HTMLDivElement>(null);
  const primaryAction = worker.running ? onOpen : onStart;
  const primaryActionLabel = worker.running ? "Open terminal" : worker.runtime_error ? "Retry worker" : "Wake worker";
  const attention = workerAttention(worker);

  useEffect(() => {
    if (worker.attention_state !== "with_operator" || worker.engagement_expires_at === undefined) return;
    const remaining = worker.engagement_expires_at * 1000 - Date.now();
    if (remaining <= 0) return;
    const timer = window.setTimeout(() => refreshAttention((current) => current + 1), remaining);
    return () => window.clearTimeout(timer);
  }, [worker.attention_state, worker.engagement_expires_at]);

  useEffect(() => {
    if (!menuOpen) return;
    function dismissMenu(event: PointerEvent) {
      if (event.target instanceof Node && !rowRef.current?.contains(event.target)) setMenuOpen(false);
    }
    document.addEventListener("pointerdown", dismissMenu);
    return () => document.removeEventListener("pointerdown", dismissMenu);
  }, [menuOpen]);

  function run(action: () => void) {
    setMenuOpen(false);
    action();
  }

  return (
    <div
      ref={rowRef}
      className={`worker-row worker-state-${attention.state}`}
      onContextMenu={(event) => {
        event.preventDefault();
        setMenuOpen(true);
      }}
      onKeyDown={(event) => {
        if (event.key === "Escape") setMenuOpen(false);
      }}
    >
      <button
        className="worker-button"
        aria-current={selected ? "page" : undefined}
        onClick={primaryAction}
        disabled={busy}
      >
        <span className="worker-avatar"><BeeMascot role={worker.role === "queen" ? "queen" : "worker"} expression={attention.expression} /></span>
        <span className="worker-copy">
          <span className="worker-copy-heading"><strong>{worker.name}</strong><span className="worker-attention-label">{attention.label}</span></span>
          <small title={detail}>{detail}</small>
        </span>
        <span className={`presence ${attention.presence}`} title={attention.label} aria-hidden="true" />
      </button>
      <button
        className="worker-menu-trigger"
        aria-label={`Actions for ${worker.name}`}
        aria-haspopup="menu"
        aria-expanded={menuOpen}
        onClick={() => setMenuOpen((current) => !current)}
        disabled={busy}
      >
        <span aria-hidden="true">•••</span>
      </button>
      {menuOpen && (
        <div className="worker-menu" role="menu" aria-label={`${worker.name} actions`}>
          <button role="menuitem" onClick={() => run(primaryAction)}>
            {primaryActionLabel}
          </button>
          {worker.running && worker.role !== "queen" && (
            <button className="danger-text" role="menuitem" onClick={() => run(onStop)}>Put worker to sleep</button>
          )}
          {worker.role === "queen" && <span className="protected-menu-note">Queen is always active</span>}
        </div>
      )}
    </div>
  );
}

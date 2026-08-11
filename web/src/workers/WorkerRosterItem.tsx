import { useEffect, useRef, useState } from "react";

import type { Worker } from "../api";
import BeeMascot from "../brand/BeeMascot";

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
  const rowRef = useRef<HTMLDivElement>(null);
  const primaryAction = worker.running ? onOpen : onStart;

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
      className="worker-row"
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
        <span className="worker-avatar"><BeeMascot role={worker.role === "queen" ? "queen" : "worker"} expression={worker.running ? "focused" : "sleeping"} /></span>
        <span className="worker-copy">
          <strong>{worker.name}</strong>
          <small>{detail}</small>
        </span>
        <span className={`presence ${worker.running ? "online" : "offline"}`} title={worker.running ? "Running" : "Stopped"} />
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
            {worker.running ? "Open terminal" : "Start worker"}
          </button>
          {worker.running && worker.role !== "queen" && (
            <button className="danger-text" role="menuitem" onClick={() => run(onStop)}>Stop worker</button>
          )}
          {worker.role === "queen" && <span className="protected-menu-note">Queen is always active</span>}
        </div>
      )}
    </div>
  );
}

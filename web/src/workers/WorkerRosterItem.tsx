import { useEffect, useState } from "react";

import type { Worker } from "../api";
import BeeMascot from "../brand/BeeMascot";
import CursorMenu, { pointFromElement, type MenuPoint } from "../shared/CursorMenu";
import { workerAttention, workerSilence } from "./workerAttention";

type Props = {
  worker: Worker;
  selected: boolean;
  detail: string;
  workSummary?: string;
  busy: boolean;
  onOpen: () => void;
  onStart: () => void;
  onStop: () => void;
};

export default function WorkerRosterItem({ worker, selected, detail, workSummary, busy, onOpen, onStart, onStop }: Props) {
  const [menuPoint, setMenuPoint] = useState<MenuPoint>();
  const [, refreshAttention] = useState(0);
  const primaryAction = worker.running ? onOpen : onStart;
  const primaryActionLabel = worker.running ? "Open terminal" : worker.runtime_error ? "Retry worker" : "Wake worker";
  const attention = workerAttention(worker);
  const silence = workerSilence(worker);

  useEffect(() => {
    if (worker.attention_state !== "with_operator" || worker.engagement_expires_at === undefined) return;
    const remaining = worker.engagement_expires_at * 1000 - Date.now();
    if (remaining <= 0) return;
    const timer = window.setTimeout(() => refreshAttention((current) => current + 1), remaining);
    return () => window.clearTimeout(timer);
  }, [worker.attention_state, worker.engagement_expires_at]);

  function run(action: () => void) {
    setMenuPoint(undefined);
    action();
  }

  return (
    <div
      className={`worker-row worker-state-${attention.state}`}
      onContextMenu={(event) => {
        event.preventDefault();
        setMenuPoint({ x: event.clientX, y: event.clientY });
      }}
      onKeyDown={(event) => {
        if (event.key === "Escape") setMenuPoint(undefined);
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
          <span className="worker-copy-heading"><strong>{worker.name}</strong><span className="worker-attention-label">{attention.label}{silence ? <span className="worker-silence"> · {silence}</span> : null}{worker.unconfirmed_delivery ? (
            <span
              className="worker-unconfirmed"
              role="img"
              aria-label="Swarm could not confirm this worker received its briefing"
              title="Swarm wrote a briefing to this worker and could not confirm it landed"
            >!</span>
          ) : null}</span></span>
          <small title={detail}>{detail}</small>
          {workSummary ? <span className="worker-work-summary" title={`Open work: ${workSummary}`}>{workSummary}</span> : null}
        </span>
        <span className={`presence ${attention.presence}`} title={attention.label} aria-hidden="true" />
      </button>
      <button
        className="worker-menu-trigger"
        aria-label={`Actions for ${worker.name}`}
        aria-haspopup="menu"
        aria-expanded={Boolean(menuPoint)}
        onClick={(event) => {
          const point = pointFromElement(event.currentTarget);
          setMenuPoint((current) => current ? undefined : point);
        }}
        disabled={busy}
      >
        <span aria-hidden="true">•••</span>
      </button>
      {menuPoint && (
        <CursorMenu className="worker-menu" point={menuPoint} onClose={() => setMenuPoint(undefined)} label={`${worker.name} actions`}>
          <button role="menuitem" onClick={() => run(primaryAction)}>
            {primaryActionLabel}
          </button>
          {worker.running && worker.role !== "queen" && (
            <button className="danger-text" role="menuitem" onClick={() => run(onStop)}>Put worker to sleep</button>
          )}
          {worker.role === "queen" && <span className="protected-menu-note">Queen is always active</span>}
        </CursorMenu>
      )}
    </div>
  );
}

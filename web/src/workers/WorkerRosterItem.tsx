import { useEffect, useState } from "react";

import { TEMPORARY_PROVIDERS, type Worker } from "../api";
import WorkerAvatar from "./WorkerAvatar";
import CursorMenu, { pointFromElement, type MenuPoint } from "../shared/CursorMenu";
import { heldForAnswer, workerAttention, workerSilence } from "./workerAttention";

type Props = {
  worker: Worker;
  selected: boolean;
  detail: string;
  workSummary?: string;
  busy: boolean;
  /**
   * Why this worker's controls are inert, when they are.
   *
   * A development rebuild disables the whole control room for as long as the
   * build runs, which is minutes. Greying out every worker with no reason
   * attached to any of them reads as the roster having broken.
   */
  busyReason?: string;
  onOpen: () => void;
  onStart: () => void;
  onStop: () => void;
  /**
   * Opens a scratch shell in this worker's workspace.
   *
   * Optional because a shell is not part of a worker's lifecycle: a caller that
   * has nowhere to put a terminal simply does not offer it, rather than being
   * handed one it cannot show.
   */
  onOpenShell?: () => void;
  /** Spawns a throwaway sibling on another provider, in the same workspace. */
  onSpawnTemporary?: (provider: string) => void;
  /** Keeps a temporary worker, under a permanent name. */
  onAdopt?: () => void;
  /** Dismisses a temporary worker. Named Release rather than Kill because its
   *  board writes survive it either way. */
  onRelease?: () => void;
};

export default function WorkerRosterItem({ worker, selected, detail, workSummary, busy, busyReason, onOpen, onStart, onStop, onOpenShell, onSpawnTemporary, onAdopt, onRelease }: Props) {
  const [menuPoint, setMenuPoint] = useState<MenuPoint>();
  const [, refreshAttention] = useState(0);
  const primaryAction = worker.running ? onOpen : onStart;
  const primaryActionLabel = worker.running ? "Open worker" : worker.runtime_error ? "Retry worker" : "Wake worker";
  const attention = workerAttention(worker);
  // A worker holding for an answer reports how long the answer has been owed,
  // not how long its terminal has been quiet. Silence age looks right for a
  // held worker by coincidence — it stopped producing output because it
  // stopped — and measures the wrong thing.
  const held = heldForAnswer(worker);
  const pausedTitle = busy
    ? `${busyReason ?? "Swarm is working"} — worker controls pause until it finishes. ${worker.name} keeps running.`
    : undefined;
  const silence = held ?? workerSilence(worker);

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
        title={pausedTitle}
      >
        <WorkerAvatar worker={worker} />
        <span className="worker-copy">
          <span className="worker-copy-heading"><strong>{worker.name}</strong><span className="worker-attention-label">{attention.label}{silence ? <span className="worker-silence"> · {silence}</span> : null}{worker.answer_overdue ? <span className="worker-overdue" title="This request passed the deadline its asker set. The worker is still holding."> · overdue</span> : null}{worker.unconfirmed_delivery ? (
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
        title={pausedTitle}
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
          {onOpenShell && (
            <button role="menuitem" onClick={() => run(onOpenShell)}>Open a shell here</button>
          )}
          {worker.ephemeral && onAdopt && (
            <button role="menuitem" onClick={() => run(onAdopt)}>Adopt into the Hive</button>
          )}
          {worker.ephemeral && onRelease && (
            <button role="menuitem" onClick={() => run(onRelease)}>Release</button>
          )}
          {!worker.ephemeral && onSpawnTemporary && TEMPORARY_PROVIDERS
            .filter((choice) => choice.provider !== worker.provider)
            .map((choice) => (
              <button key={choice.provider} role="menuitem" onClick={() => run(() => onSpawnTemporary(choice.provider))}>
                Try this with {choice.label}{choice.alpha ? " (alpha)" : ""}
              </button>
            ))}
          {worker.running && worker.role !== "queen" && (
            <button className="danger-text" role="menuitem" onClick={() => run(onStop)}>Put worker to sleep</button>
          )}
          {worker.role === "queen" && <span className="protected-menu-note">Queen is always active</span>}
        </CursorMenu>
      )}
    </div>
  );
}

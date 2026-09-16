import type { Worker } from "../api";

type Props = {
  workers: Pick<Worker, "id" | "name" | "runtime_error">[];
  onOpen: (workerId: string) => void;
};

/**
 * Workers that cannot run, and why, where the operator will actually see it.
 *
 * ⚠️ THE GAP THIS CLOSES COST TEN MINUTES OF A DEAD QUEEN. When a worker's
 * automatic recovery gives up it records a runtime error, which renders as a
 * Blocked chip on the roster and a WARN in the journal — and nothing else. The
 * operator found out by noticing the chip and asking what it meant. At three in
 * the morning nobody is looking at the roster, and when the worker in question
 * is Queen the entire coordination layer is stopped: no routing, no review, no
 * dispatch, silently.
 *
 * ONE CARD HOWEVER MANY WORKERS, like the held-delivery and return-recovery
 * cards beside it. The count on the badge has to agree with the page, because a
 * badge that disagrees teaches the operator to stop believing the badge.
 */
export default function WorkerCannotRunCard({ workers, onOpen }: Props) {
  const stopped = workers.filter((worker) => worker.runtime_error);
  if (stopped.length === 0) return null;
  return <section className="queen-attention-card held-delivery-card" aria-label="A worker cannot run">
    <div>
      <p className="eyebrow">Stopped</p>
      <h3>{stopped.length === 1 ? "A worker cannot run" : `${stopped.length} workers cannot run`}</h3>
      <ul>{stopped.map((worker) => <li key={worker.id}>
        <strong>{worker.name}</strong>{": "}{worker.runtime_error}
      </li>)}</ul>
      {/* Said plainly, because the difference between "it will come back" and
          "it will not" is the whole reason this card exists. */}
      <p>Automatic restarts have stopped for these workers. They stay down until you start them.</p>
    </div>
    <button type="button" onClick={() => stopped[0] && onOpen(stopped[0].id)}>Open worker</button>
  </section>;
}

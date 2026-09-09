import type { Worker } from "../api";

type Props = {
  workers: Pick<Worker, "id" | "name" | "return_attention">[];
  onReview: () => void;
};

export default function WorkerReturnAttentionCard({ workers, onReview }: Props) {
  const affected = workers.filter((worker) => worker.return_attention);
  if (affected.length === 0) return null;
  return <section className="queen-attention-card held-delivery-card" aria-label="Worker return needs checking">
    <div>
      <p className="eyebrow">After maintenance</p>
      <h3>{affected.length === 1 ? "A worker needs help returning" : `${affected.length} workers need help returning`}</h3>
      <ul>{affected.map((worker) => <li key={worker.id}>
        <strong>{worker.name}</strong>{": "}{worker.return_attention === "failed"
          ? "The engine could not start this worker."
          : "Startup was not confirmed. Check existing sessions before retrying."}
      </li>)}</ul>
      <p>Automatic retries are paused to avoid duplicate sessions.</p>
    </div>
    <button type="button" onClick={onReview}>Review recovery</button>
  </section>;
}

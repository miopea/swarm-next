import type { EngineUpdateAttention } from "./engineAutoUpdate";

type Props = {
  attention: EngineUpdateAttention | null;
  onReview: () => void;
};

/**
 * A waiting worker engine update in Needs you (ADR 0156): after an automatic
 * try stopped partway, naming the workers it stopped, or after a day of
 * waiting. Never forced; the worker engine card stays the manual route.
 */
export default function WorkerEngineAttentionCard({ attention, onReview }: Props) {
  if (!attention) return null;
  return <section className="queen-attention-card held-delivery-card" aria-label="Worker engine update needs you">
    <div>
      <p className="eyebrow">Worker engine</p>
      {attention.kind === "stopped_partway" ? <>
        <h3>The automatic engine update stopped partway</h3>
        <p>{attention.detail}</p>
        {attention.stopped.length > 0 ? <ul>{attention.stopped.map((name) => <li key={name}>
          <strong>{name}</strong>{": stopped, and comes back once the engine is updated."}
        </li>)}</ul> : null}
        <p>It is not tried again by itself. Update the engine from Settings when it suits you.</p>
      </> : <>
        <h3>{`The worker engine update has waited ${attention.hours} hours`}</h3>
        <p>{!attention.enabled
          ? "Automatic engine updates are off, so it waits for you."
          : attention.reason
            ? `It applies by itself once every worker is resting. Not yet: ${attention.reason}.`
            : "It applies by itself once every worker is resting, and they have not all rested at once."}</p>
        <p>Updating it from Settings restarts the loaded workers.</p>
      </>}
    </div>
    <button type="button" onClick={onReview}>Open worker engine</button>
  </section>;
}

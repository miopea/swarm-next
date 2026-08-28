import type { Worker } from "../api";
import BeeMascot from "../brand/BeeMascot";
import { resolveMark } from "../brand/beeMarks";
import { workerAttention } from "./workerAttention";

type Props = {
  worker: Worker;
  /** Adds the presence pill beside the bee, for surfaces that show one. */
  presence?: boolean;
};

/**
 * A worker's bee, drawn the same way everywhere it appears.
 *
 * WHY THIS EXISTS. Three facts decide how a worker's bee looks — its role, its
 * attention state and its mark — and every surface that drew one was assembling
 * those itself. The mobile picker passed only the expression, so every worker
 * there wore the default bee and the Queen was drawn as a worker; the desktop
 * rail passed all three. The marks exist precisely so one repository's worker
 * can be told from another's at a glance, and they reached exactly one of the
 * places a worker is normally seen.
 *
 * Measured 2026-08-28: 23 BeeMascot call sites, 3 passing a mark, and only one
 * of those a worker in a list. The rest are empty states and decorations, which
 * correctly have no worker and no mark — this component is for the ones that
 * DO name a worker, and it is the whole reason those cannot drift apart again.
 *
 * ROLE IS DERIVED, NOT PASSED. A caller that forgets it draws the Queen as a
 * worker, which is the same defect efa4ee4 fixed for the control room and which
 * the Queen's own attention card still has. Taking the worker rather than its
 * parts is what removes that whole class of caller mistake.
 */
export default function WorkerAvatar({ worker, presence = false }: Props) {
  const attention = workerAttention(worker);
  return (
    <>
      <span className="worker-avatar">
        <BeeMascot
          role={worker.role === "queen" ? "queen" : "worker"}
          expression={attention.expression}
          mark={resolveMark(worker.id, worker.mark)}
        />
      </span>
      {presence ? (
        <span className={`presence ${attention.presence}`} title={attention.label} aria-hidden="true" />
      ) : null}
    </>
  );
}

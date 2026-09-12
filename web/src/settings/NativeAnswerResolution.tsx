import { useEffect, useState } from "react";
import { fetchNativeAnswerResolution, setNativeAnswerResolution } from "../api";

/**
 * Whether answering in a worker's own terminal settles the question in Needs You.
 *
 * ⚠️ THE KILL SWITCH FOR A FEATURE THAT ACTS ON THE OPERATOR'S BEHALF. When this
 * is on, an answer typed into a terminal resolves a decision and is recorded as
 * the operator's word. That is the point of it, and it is also why there has to
 * be a way to stop it that a worker cannot reach — the server requires an
 * operator credential for the write, because every worker runs on this machine
 * and local requests are otherwise trusted.
 *
 * Default on, chosen by the operator against the author's recommendation.
 */
export default function NativeAnswerResolution({ operatorToken }: { operatorToken: string }) {
  const [enabled, setEnabled] = useState<boolean | undefined>(undefined);
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    let current = true;
    void (async () => {
      try {
        const status = await fetchNativeAnswerResolution(operatorToken);
        if (current) setEnabled(status.enabled);
      } catch {
        if (current) setFailed(true);
      }
    })();
    return () => { current = false; };
  }, [operatorToken]);

  async function choose(next: boolean) {
    setBusy(true);
    setFailed(false);
    try {
      const status = await setNativeAnswerResolution(operatorToken, next);
      setEnabled(status.enabled);
    } catch {
      // The switch is NOT moved optimistically. A toggle that flips on screen
      // and not on the Hive is worse than one that refuses: the operator would
      // believe they had turned this off.
      setFailed(true);
    } finally {
      setBusy(false);
    }
  }

  return (
    <section id="settings-answers" className="settings-card" aria-labelledby="native-answers-heading">
      <div>
        <p className="eyebrow">Terminal answers</p>
        <h3 id="native-answers-heading">Answering in a worker's terminal</h3>
      </div>
      <p>
        When this is on, answering a question in a worker's own terminal settles
        the matching item in What needs you, and your answer is recorded as
        yours. Swarm only does this when the engine confirms the answer was
        completed and exactly one open question matches it; anything else leaves
        the item open and says why.
      </p>
      <label className="queen-automation-toggle">
        <input
          type="checkbox"
          checked={enabled ?? false}
          disabled={busy || enabled === undefined}
          onChange={(event) => void choose(event.target.checked)}
        />
        <span>{enabled === false ? "Off — answer in Swarm" : "On — terminal answers count"}</span>
      </label>
      {enabled === undefined && !failed ? <small role="status">Reading the current setting…</small> : null}
      {failed ? (
        <small role="alert">
          That could not be changed, so the setting is unchanged. Changing it
          needs your operator credential — a worker on this machine cannot.
        </small>
      ) : null}
    </section>
  );
}

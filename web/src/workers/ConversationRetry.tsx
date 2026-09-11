import { useEffect, useRef, useState } from "react";

/**
 * A retry button that says what happened.
 *
 * ⚠️ THE BUTTON IT REPLACES WORKED. It called the same refresh the 120-second
 * poll calls, the request went out, the results came back — and when nothing
 * had changed, nothing on screen changed either. Operator, 2026-09-11: "The
 * retry buttons don't seem to do anything."
 *
 * A control that fires and finds nothing is indistinguishable from one that is
 * not wired, and the remedy for those two is opposite: one needs feedback, the
 * other needs code. So this reports the outcome rather than changing it.
 *
 * The underlying refresh coalesces concurrent calls and resolves without
 * telling us whether anything differed, so "Checked just now" is an honest
 * claim about the check having run — not a claim that something was found.
 */
export default function ConversationRetry({ onRetry }: { onRetry: () => Promise<void> }) {
  const [state, setState] = useState<"idle" | "checking" | "checked" | "failed">("idle");
  // Survives the component being re-rendered by the very refresh it triggered.
  const alive = useRef(true);
  useEffect(() => () => { alive.current = false; }, []);

  async function run() {
    setState("checking");
    try {
      await onRetry();
      if (alive.current) setState("checked");
    } catch {
      if (alive.current) setState("failed");
    }
  }

  return (
    <div className="conversation-retry">
      <button
        type="button"
        className="runtime-update-run"
        disabled={state === "checking"}
        onClick={() => void run()}
      >
        {state === "checking" ? "Checking…" : "Retry conversation checks"}
      </button>
      {state === "checked" ? (
        <small role="status">Checked just now. Anything still listed was re-read and is unchanged.</small>
      ) : null}
      {state === "failed" ? (
        <small role="status">That check could not complete. The results below may be out of date.</small>
      ) : null}
    </div>
  );
}

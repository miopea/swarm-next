import { useCallback, useId, useState } from "react";
import { fetchCheckGate, setCheckGate, type CheckGate, type CheckGateLevel } from "../api";
import { RuntimeRequestError } from "../api/request";
import { useVisiblePolling } from "../runtime/useVisiblePolling";

const LEVELS: { level: CheckGateLevel; label: string; detail: string }[] = [
  { level: "off", label: "Off", detail: "CI plays no part in closing work." },
  { level: "warn", label: "Show CI", detail: "Work waiting on review shows its CI, and can still close." },
  { level: "require", label: "Require passing CI", detail: "Work that recorded commits can't close while its CI fails or is still running. CI that can't be read doesn't hold it, and the task's history says why." },
];

/**
 * The check gate (ADR 0115): whether CI must pass on a task's commits before
 * the work counts as done. CI is read through this machine's own gh login.
 */
export default function CheckGateSettings({ operatorToken }: { operatorToken: string }) {
  const [gate, setGate] = useState<CheckGate>();
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string>();
  const lockedId = useId();
  const load = useCallback(async (signal: AbortSignal) => {
    try {
      setGate(await fetchCheckGate(operatorToken, signal));
    } catch {
      if (!signal.aborted) setError("Could not load the check gate. It will retry.");
    }
  }, [operatorToken]);
  useVisiblePolling(load, true, null);

  async function choose(level: CheckGateLevel) {
    setSaving(true);
    setError(undefined);
    try {
      setGate(await setCheckGate(operatorToken, level));
    } catch (failure) {
      setError(failure instanceof RuntimeRequestError && failure.status === 409
        ? "Your Apiary sets this, and only its Keeper can change it."
        : "The check gate was not saved. Try again.");
    } finally {
      setSaving(false);
    }
  }

  const locked = Boolean(gate?.set_by_apiary);
  const current = LEVELS.find((entry) => entry.level === gate?.level);
  return (
    <section id="settings-check-gate" className="settings-card check-gate-settings" aria-labelledby="check-gate-heading">
      <div><p className="eyebrow">Check gate</p><h3 id="check-gate-heading">When work counts as done</h3></div>
      <p>Swarm reads GitHub CI for the commits a task recorded, through this machine&apos;s gh login. Tasks without commits are never held.</p>
      <label htmlFor="check-gate-level"><span>Level</span>
        <select
          id="check-gate-level"
          value={gate?.level ?? "off"}
          disabled={!gate || saving || locked}
          aria-describedby={locked ? lockedId : undefined}
          onChange={(event) => void choose(event.target.value as CheckGateLevel)}
        >
          {LEVELS.map((entry) => <option key={entry.level} value={entry.level}>{entry.label}</option>)}
        </select>
      </label>
      {locked && <small id={lockedId} className="check-gate-set-by-apiary">Set by your Apiary. Only its Keeper can change this.</small>}
      {current && <small className="check-gate-detail">{current.detail}</small>}
      {error && <p role="alert">{error}</p>}
    </section>
  );
}

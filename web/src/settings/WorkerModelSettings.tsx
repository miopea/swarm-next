import { useCallback, useId, useState } from "react";
import { fetchWorkerModel, setWorkerModel, type WorkerModel, type WorkerModelSetting } from "../api";
import { RuntimeRequestError } from "../api/request";
import { useVisiblePolling } from "../runtime/useVisiblePolling";
import { WORKER_MODEL_LABELS } from "./workerModelLabels";

const DETAILS: Record<WorkerModel, string> = {
  default: "Swarm names no model, and Claude Code uses its own default.",
  opus: "Every repository worker starts on Opus, the most capable and the most expensive.",
  sonnet: "Every repository worker starts on Sonnet, which costs less than Opus.",
  haiku: "Every repository worker starts on Haiku, the cheapest and the least capable.",
};
const MODELS = Object.keys(WORKER_MODEL_LABELS) as WorkerModel[];

/**
 * The worker model (ADR 0116): the one Claude model every repository worker on
 * this Hive starts with. Queen keeps her own default. A change reaches each
 * worker when it next starts; nothing is restarted for it.
 */
export default function WorkerModelSettings({ operatorToken }: { operatorToken: string }) {
  const [setting, setSetting] = useState<WorkerModelSetting>();
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string>();
  const lockedId = useId();
  const load = useCallback(async (signal: AbortSignal) => {
    try {
      setSetting(await fetchWorkerModel(operatorToken, signal));
    } catch {
      if (!signal.aborted) setError("Could not load the worker model. It will retry.");
    }
  }, [operatorToken]);
  useVisiblePolling(load, true, null);

  async function choose(model: WorkerModel) {
    setSaving(true);
    setError(undefined);
    try {
      setSetting(await setWorkerModel(operatorToken, model));
    } catch (failure) {
      setError(failure instanceof RuntimeRequestError && failure.status === 409
        ? "Your Apiary sets this, and only its Keeper can change it."
        : "The worker model was not saved. Try again.");
    } finally {
      setSaving(false);
    }
  }

  const locked = Boolean(setting?.set_by_apiary);
  const short = setting?.workers_short ?? 0;
  return (
    <section id="settings-worker-model" className="settings-card worker-model-settings" aria-labelledby="worker-model-heading">
      <div><p className="eyebrow">Worker model</p><h3 id="worker-model-heading">What repository workers run on</h3></div>
      <p>Every Claude repository worker on this Hive starts on this model. Queen keeps her own default, and Codex workers are not affected. A change reaches each worker when it next starts.</p>
      <label htmlFor="worker-model-choice"><span>Model</span>
        <select
          id="worker-model-choice"
          value={setting?.model ?? "default"}
          disabled={!setting || saving || locked}
          aria-describedby={locked ? lockedId : undefined}
          onChange={(event) => void choose(event.target.value as WorkerModel)}
        >
          {MODELS.map((model) => <option key={model} value={model}>{WORKER_MODEL_LABELS[model]}</option>)}
        </select>
      </label>
      {locked && <small id={lockedId} className="worker-model-set-by-apiary">Set by your Apiary. Only its Keeper can change this.</small>}
      {setting && <small className="worker-model-detail">{DETAILS[setting.model]}</small>}
      {short > 0 && setting && (
        <small className="worker-model-short" role="status">
          {short} {short === 1 ? "worker is" : "workers are"} still on another model, and {short === 1 ? "switches" : "switch"} to {WORKER_MODEL_LABELS[setting.model]} when {short === 1 ? "it next starts" : "they next start"}. The left panel lists {short === 1 ? "it" : "them"} and can restart {short === 1 ? "it" : "them"} now.
        </small>
      )}
      {error && <p role="alert">{error}</p>}
    </section>
  );
}

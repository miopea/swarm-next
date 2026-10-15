import { useCallback, useId, useState } from "react";
import { fetchTerminalHistory, setTerminalHistory, type HistoryRetentionDays, type TerminalHistory } from "../api";
import { RuntimeRequestError } from "../api/request";
import { useVisiblePolling } from "../runtime/useVisiblePolling";

const CHOICES: HistoryRetentionDays[] = [1, 3, 7, 14, 30];
const days = (count: number) => `${count} ${count === 1 ? "day" : "days"}`;

/** What the terminal host is doing about the setting, in words. */
function hostLine(history: TerminalHistory): { text: string; waiting: boolean } {
  switch (history.host) {
    case "in_effect": return { text: "In effect.", waiting: false };
    case "applying": return {
      text: history.host_max_age_seconds
        ? `The terminal host still keeps ${days(Math.round(history.host_max_age_seconds / 86_400))}, and takes this within a minute.`
        : "The terminal host takes this within a minute.",
      waiting: true,
    };
    case "awaiting_engine_update": return { text: "Takes effect after the worker engine update, which runs by itself once your workers are idle.", waiting: true };
    case "unknown": return { text: "Checking what the terminal host keeps…", waiting: true };
  }
}

/**
 * Terminal history retention (ADR 0118): how many days of every worker's
 * terminal output this Hive keeps. The terminal host applies it while it runs;
 * "In effect" is the host's own word, never assumed.
 */
export default function TerminalHistorySettings({ operatorToken }: { operatorToken: string }) {
  const [history, setHistory] = useState<TerminalHistory>();
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string>();
  const lockedId = useId();
  const load = useCallback(async (signal: AbortSignal) => {
    try {
      setHistory(await fetchTerminalHistory(operatorToken, signal));
    } catch {
      if (!signal.aborted) setError("Could not load terminal history retention. It will retry.");
    }
  }, [operatorToken]);
  useVisiblePolling(load, true, null);

  async function choose(value: HistoryRetentionDays) {
    setSaving(true);
    setError(undefined);
    try {
      setHistory(await setTerminalHistory(operatorToken, value));
    } catch (failure) {
      setError(failure instanceof RuntimeRequestError && failure.status === 409
        ? "Your Apiary sets this, and only its Keeper can change it."
        : "Terminal history retention was not saved. Try again.");
    } finally {
      setSaving(false);
    }
  }

  const locked = Boolean(history?.set_by_apiary);
  const host = history ? hostLine(history) : undefined;
  return (
    <section id="settings-terminal-history" className="settings-card terminal-history-settings" aria-labelledby="terminal-history-heading">
      <div><p className="eyebrow">Terminal history</p><h3 id="terminal-history-heading">How long workers&apos; terminals are kept</h3></div>
      <p>Swarm keeps every worker&apos;s terminal output so you can scroll back after a restart. Older history is deleted, and shortening this deletes it straight away. A busy Hive may keep less, because history is also capped at 512 MB in all.</p>
      <label htmlFor="terminal-history-days"><span>Keep</span>
        <select
          id="terminal-history-days"
          value={history?.days ?? 7}
          disabled={!history || saving || locked}
          aria-describedby={locked ? lockedId : undefined}
          onChange={(event) => void choose(Number(event.target.value) as HistoryRetentionDays)}
        >
          {CHOICES.map((choice) => <option key={choice} value={choice}>{days(choice)}</option>)}
        </select>
      </label>
      {locked && <small id={lockedId} className="terminal-history-set-by-apiary">Set by your Apiary. Only its Keeper can change this.</small>}
      {host && <small className={host.waiting ? "terminal-history-waiting" : "terminal-history-detail"} role="status">{host.text}</small>}
      {error && <p role="alert">{error}</p>}
    </section>
  );
}

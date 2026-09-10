import { useCallback, useEffect, useRef, useState } from "react";
import type { DecisionClarification, DecisionRequest } from "../api";

export type ReadClarifications = (decisionId: string, signal: AbortSignal) => Promise<DecisionClarification[]>;
type Entry = { revision: string; history: DecisionClarification[]; error?: string };
const MAX_CACHED_CONVERSATIONS = 8;
const EMPTY_HISTORY: DecisionClarification[] = [];

function revisionOf(decision: DecisionRequest): string {
  return JSON.stringify([decision.state, decision.clarification ?? null]);
}

/** One owned read, eight bounded histories; no per-card polling. */
export function useDecisionClarifications(decisions: DecisionRequest[], read?: ReadClarifications) {
  const latestRead = useRef(read);
  latestRead.current = read;
  const [selected, setSelected] = useState<string>();
  const [request, setRequest] = useState(0);
  const [loading, setLoading] = useState<string>();
  const [cache, setCache] = useState<Map<string, Entry>>(new Map());
  const decision = decisions.find(item => item.id === selected);
  const revision = decision ? revisionOf(decision) : undefined;
  const enabled = Boolean(read);

  useEffect(() => {
    if (!selected || !revision || !enabled) return;
    const controller = new AbortController();
    let disposed = false;
    setLoading(selected);
    const remember = (entry: Entry) => {
      if (disposed) return;
      setCache(previous => {
        const next = new Map(previous);
        next.delete(selected);
        next.set(selected, entry);
        while (next.size > MAX_CACHED_CONVERSATIONS) next.delete(next.keys().next().value!);
        return next;
      });
    };
    const ceiling = setTimeout(() => {
      controller.abort();
      remember({ revision, history: EMPTY_HISTORY, error: "The conversation took too long to load. Try again." });
      setLoading(current => current === selected ? undefined : current);
    }, 15_000);
    void latestRead.current!(selected, controller.signal).then(history => {
      if (controller.signal.aborted || disposed) return;
      if (history.length > 32 || history.some(round => round.decision_id !== selected)) {
        throw new Error("The conversation response did not match this decision. Try again.");
      }
      remember({ revision, history });
    }).catch(error => {
      if (!controller.signal.aborted) remember({ revision, history: EMPTY_HISTORY,
        error: error instanceof Error ? error.message : "The conversation could not be loaded. Try again." });
    }).finally(() => {
      clearTimeout(ceiling);
      if (!disposed) setLoading(current => current === selected ? undefined : current);
    });
    return () => { disposed = true; clearTimeout(ceiling); controller.abort(); };
  }, [selected, revision, request, enabled]);

  useEffect(() => {
    const ids = new Set(decisions.map(item => item.id));
    setCache(previous => {
      if ([...previous.keys()].every(id => ids.has(id))) return previous;
      return new Map([...previous].filter(([id]) => ids.has(id)));
    });
    if (selected && !ids.has(selected)) { setSelected(undefined); setLoading(undefined); }
  }, [decisions, selected]);

  const reload = useCallback((id: string) => { setSelected(id); setRequest(value => value + 1); }, []);
  const forDecision = (item: DecisionRequest) => {
    const entry = cache.get(item.id);
    const current = entry?.revision === revisionOf(item) ? entry : undefined;
    return { history: current?.history ?? EMPTY_HISTORY, loadError: current?.error,
      loading: loading === item.id, loaded: Boolean(current && !current.error) };
  };
  return { reload, forDecision };
}

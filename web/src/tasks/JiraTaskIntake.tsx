import { useEffect, useMemo, useState } from "react";

import {
  fetchJiraBindingIssues,
  fetchJiraBindings,
  fetchJiraReadiness,
  syncJiraBinding,
  type JiraIssue,
  type JiraProjectBinding,
  type JiraReadiness,
} from "../api";

type Props = {
  operatorToken: string;
  onImported: () => Promise<void>;
};

export default function JiraTaskIntake({ operatorToken, onImported }: Props) {
  const [readiness, setReadiness] = useState<JiraReadiness>();
  const [bindings, setBindings] = useState<JiraProjectBinding[]>([]);
  const [activeBinding, setActiveBinding] = useState<JiraProjectBinding>();
  const [issues, setIssues] = useState<JiraIssue[]>([]);
  const [query, setQuery] = useState("");
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");

  useEffect(() => {
    let cancelled = false;
    void Promise.all([fetchJiraReadiness(operatorToken), fetchJiraBindings(operatorToken)])
      .then(([nextReadiness, nextBindings]) => {
        if (cancelled) return;
        setReadiness(nextReadiness);
        setBindings(nextReadiness.connection === "ready" ? nextBindings.filter((binding) => binding.workflow_mapped) : []);
      })
      .catch(() => { if (!cancelled) { setReadiness(undefined); setBindings([]); } });
    return () => { cancelled = true; };
  }, [operatorToken]);

  const visibleIssues = useMemo(() => {
    const normalized = query.trim().toLocaleLowerCase();
    if (!normalized) return issues;
    return issues.filter((issue) => [issue.key, issue.summary, issue.status_name]
      .some((value) => value.toLocaleLowerCase().includes(normalized)));
  }, [issues, query]);

  if (readiness?.connection !== "ready" || bindings.length === 0) return null;

  async function browse(binding: JiraProjectBinding) {
    setBusy(true);
    setMessage("");
    try {
      const next = await fetchJiraBindingIssues(operatorToken, binding.id);
      setActiveBinding(binding);
      setIssues(next);
      setQuery("");
      setSelectedIds(new Set());
      setMessage(next.length ? "Choose available work to claim for this Hive." : `No unassigned open work is available to claim in ${binding.project_name}.`);
    } catch (error) {
      setMessage(error instanceof Error ? error.message : "Jira work could not be loaded.");
    } finally {
      setBusy(false);
    }
  }

  async function importSelected() {
    if (!activeBinding || selectedIds.size === 0) return;
    setBusy(true);
    setMessage("");
    try {
      const tasks = await syncJiraBinding(operatorToken, activeBinding.id, [...selectedIds]);
      await onImported();
      setIssues((current) => current.filter((issue) => !selectedIds.has(issue.id)));
      setSelectedIds(new Set());
      setMessage(`${tasks.length} Jira issue${tasks.length === 1 ? "" : "s"} added or refreshed on this board.`);
    } catch (error) {
      setMessage(error instanceof Error ? error.message : "Selected Jira work could not be imported.");
    } finally {
      setBusy(false);
    }
  }

  function close() {
    setActiveBinding(undefined);
    setIssues([]);
    setQuery("");
    setSelectedIds(new Set());
    setMessage("");
  }

  return (
    <section className="jira-task-source" aria-labelledby="jira-work-heading">
      <div className="jira-task-source-heading">
        <div><p className="eyebrow">Jira work</p><h3 id="jira-work-heading">Choose unassigned work from Jira</h3></div>
        {!activeBinding ? <span>Unassigned · open only</span> : null}
      </div>

      {!activeBinding ? (
        <div className="jira-project-actions" aria-label="Connected Jira projects">
          {bindings.map((binding) => (
            <button className="secondary-button" key={binding.id} type="button" disabled={busy} onClick={() => void browse(binding)}>
              <strong>{binding.project_key}</strong><span>Choose work</span>
            </button>
          ))}
        </div>
      ) : (
        <section className="jira-intake" aria-label={`Choose ${activeBinding.project_name} work`}>
          <div className="jira-intake-heading">
            <span><strong>{activeBinding.project_key} · {activeBinding.project_name}</strong><small>Unassigned · open only</small></span>
            <button className="text-button" type="button" onClick={close}>Close</button>
          </div>
          <p className="privacy-note">Choose work to claim. Swarm assigns it to {readiness.account_name ?? "you"} in Jira, then begins two-way synchronization. Worker assignment happens on the board.</p>
          <label className="jira-intake-filter">
            <span>Find an issue</span>
            <input value={query} placeholder="Key, title, or status" autoComplete="off" onChange={(event) => setQuery(event.target.value)} />
          </label>
          <div className="jira-intake-actions">
            <span>{visibleIssues.length} shown · {selectedIds.size} selected</span>
            <button className="text-button" type="button" disabled={visibleIssues.length === 0} onClick={() => setSelectedIds(new Set(visibleIssues.slice(0, 100).map((issue) => issue.id)))}>Select shown</button>
            <button className="text-button" type="button" disabled={selectedIds.size === 0} onClick={() => setSelectedIds(new Set())}>Clear</button>
          </div>
          <div className="jira-issue-list">
            {visibleIssues.map((issue) => (
              <label className="jira-issue-row" key={issue.id}>
                <input
                  type="checkbox"
                  checked={selectedIds.has(issue.id)}
                  disabled={!selectedIds.has(issue.id) && selectedIds.size >= 100}
                  onChange={(event) => setSelectedIds((current) => {
                    const next = new Set(current);
                    if (event.target.checked) next.add(issue.id); else next.delete(issue.id);
                    return next;
                  })}
                />
                <span><strong>{issue.key} · {issue.summary}</strong><small>{issue.status_name} · {issue.assignee_name ?? "Available to claim"}</small></span>
              </label>
            ))}
            {visibleIssues.length === 0 ? <p className="jira-intake-empty">No unassigned open Jira issues match this view.</p> : null}
          </div>
          <button className="primary-action" type="button" disabled={busy || selectedIds.size === 0} onClick={() => void importSelected()}>
            {busy ? "Adding work…" : `Add ${selectedIds.size} to this board`}
          </button>
        </section>
      )}
      {message ? <p className="settings-message" role="status">{message}</p> : null}
    </section>
  );
}

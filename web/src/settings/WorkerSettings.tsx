import { useState, type DragEvent, type FormEvent, type KeyboardEvent } from "react";

import type { ProviderCapabilities, ProviderKind, Worker, WorkspaceChoice } from "../api";
import BeeMascot from "../brand/BeeMascot";

type Props = {
  workers: Worker[];
  workspaces: WorkspaceChoice[];
  busy: boolean;
  providers: ProviderCapabilities;
  onCreate: (name: string, workspace: string, provider: ProviderKind) => Promise<void>;
  onUpdate: (workerId: string, name: string, autostart: boolean) => Promise<void>;
  onReorder: (workerIds: string[]) => Promise<void>;
};

export default function WorkerSettings({ workers, workspaces, busy, providers, onCreate, onUpdate, onReorder }: Props) {
  const roster = workers.filter((worker) => worker.role !== "queen");
  const available = workspaces.filter((workspace) => !workspace.configured_worker_id);
  const [name, setName] = useState("");
  const [workspace, setWorkspace] = useState("");
  const [provider, setProvider] = useState<ProviderKind>("claude_code");
  const [workspaceOpen, setWorkspaceOpen] = useState(false);
  const [highlightedWorkspace, setHighlightedWorkspace] = useState(0);
  const [draggedWorkerId, setDraggedWorkerId] = useState<string>();
  const matchingWorkspaces = workspaceMatches(available, workspace).slice(0, 8);

  async function submit(event: FormEvent) {
    event.preventDefault();
    if (!name.trim() || !workspace) return;
    await onCreate(name, workspace, provider);
    setName("");
    setWorkspace("");
    setWorkspaceOpen(false);
  }

  function selectWorkspace(choice: WorkspaceChoice) {
    setWorkspace(choice.path);
    setWorkspaceOpen(false);
    setHighlightedWorkspace(0);
  }

  function workspaceKeyDown(event: KeyboardEvent<HTMLInputElement>) {
    if (event.key === "ArrowDown") {
      event.preventDefault();
      setWorkspaceOpen(true);
      setHighlightedWorkspace((current) => Math.min(current + 1, Math.max(0, matchingWorkspaces.length - 1)));
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      setWorkspaceOpen(true);
      setHighlightedWorkspace((current) => Math.max(0, current - 1));
    } else if (event.key === "Enter" && workspaceOpen && matchingWorkspaces.length > 0) {
      event.preventDefault();
      selectWorkspace(matchingWorkspaces[highlightedWorkspace] ?? matchingWorkspaces[0]);
    } else if (event.key === "Escape") {
      setWorkspaceOpen(false);
    }
  }

  function move(index: number, offset: -1 | 1) {
    const target = index + offset;
    if (target < 0 || target >= roster.length) return;
    const ids = roster.map((worker) => worker.id);
    [ids[index], ids[target]] = [ids[target], ids[index]];
    void onReorder(ids);
  }

  function dropBefore(targetWorkerId: string, event: DragEvent) {
    event.preventDefault();
    if (!draggedWorkerId || draggedWorkerId === targetWorkerId) return;
    const ids = roster.map((worker) => worker.id).filter((workerId) => workerId !== draggedWorkerId);
    ids.splice(ids.indexOf(targetWorkerId), 0, draggedWorkerId);
    setDraggedWorkerId(undefined);
    void onReorder(ids);
  }

  return (
    <section className="settings-card worker-settings" aria-labelledby="worker-settings-heading">
      <div><p className="eyebrow">Worker roster</p><h3 id="worker-settings-heading">Your familiar crew</h3></div>
      <p>Workers remember their repository and provider conversation across stops, updates, and reboots. Reorder them to match how you work.</p>
      <div className="configured-workers">
        {workers.find((worker) => worker.role === "queen") && (
          <div className="configured-worker queen-worker">
            <span className="worker-settings-bee"><BeeMascot expression="available" role="queen" /></span>
            <span><strong>Queen</strong><small>Pinned · always active</small></span>
          </div>
        )}
        {roster.map((worker, index) => (
          <WorkerPreferenceRow
            key={worker.id}
            worker={worker}
            busy={busy}
            first={index === 0}
            last={index === roster.length - 1}
            dragging={draggedWorkerId === worker.id}
            onMove={(offset) => move(index, offset)}
            onUpdate={onUpdate}
            onDragStart={() => setDraggedWorkerId(worker.id)}
            onDragEnd={() => setDraggedWorkerId(undefined)}
            onDrop={(event) => dropBefore(worker.id, event)}
          />
        ))}
        {roster.length === 0 && <p className="empty-worker-settings">No repository workers configured yet.</p>}
      </div>
      <form className="configure-worker-form" onSubmit={(event) => void submit(event)}>
        <div className="field-stack">
          <label htmlFor="configured-worker-name">Worker name</label>
          <input id="configured-worker-name" value={name} onChange={(event) => setName(event.target.value)} placeholder="Daisy" maxLength={80} />
        </div>
        <div className="field-stack provider-field">
          <label htmlFor="configured-worker-provider">Coding provider</label>
          <select id="configured-worker-provider" value={provider} onChange={(event) => setProvider(event.target.value as ProviderKind)}>
            <option value="claude_code" disabled={!providers.claude_code}>Claude Code{providers.claude_code ? "" : " · unavailable"}</option>
            <option value="codex" disabled={!providers.codex}>Codex{providers.codex ? "" : " · waiting for maintenance"}</option>
          </select>
          <small>{providers.codex ? "Codex is ready for new repository-owned workers." : "Codex is installed and authenticated; it unlocks after the terminal host's next zero-session maintenance update."}</small>
        </div>
        <div className="field-stack">
          <label htmlFor="configured-worker-repository">Repository path</label>
          <div
            className="workspace-combobox"
            onBlur={(event) => {
              if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setWorkspaceOpen(false);
            }}
          >
            <input
              id="configured-worker-repository"
              role="combobox"
              aria-autocomplete="list"
              aria-controls="workspace-suggestions"
              aria-expanded={workspaceOpen}
              aria-activedescendant={workspaceOpen && matchingWorkspaces.length > 0 ? `workspace-option-${highlightedWorkspace}` : undefined}
              value={workspace}
              placeholder="/home/bschleifer/projects/..."
              autoComplete="off"
              spellCheck={false}
              onFocus={() => setWorkspaceOpen(true)}
              onChange={(event) => {
                setWorkspace(event.target.value);
                setWorkspaceOpen(true);
                setHighlightedWorkspace(0);
              }}
              onKeyDown={workspaceKeyDown}
            />
            {workspaceOpen && workspace.trim() && (
              <div id="workspace-suggestions" className="workspace-suggestions" role="listbox" aria-label="Repository path suggestions">
                {matchingWorkspaces.map((choice, index) => (
                  <button
                    id={`workspace-option-${index}`}
                    key={choice.path}
                    type="button"
                    role="option"
                    aria-selected={index === highlightedWorkspace}
                    className={index === highlightedWorkspace ? "workspace-suggestion highlighted" : "workspace-suggestion"}
                    onMouseDown={(event) => event.preventDefault()}
                    onClick={() => selectWorkspace(choice)}
                  >
                    <span className="workspace-suggestion-copy"><strong>{choice.name}</strong><small>{choice.path}</small></span>
                    <small className="workspace-kind">{choice.kind === "repository" ? "Repository" : "Folder"}</small>
                  </button>
                ))}
                {matchingWorkspaces.length === 0 && <p>No suggestion yet. Keep typing an existing path inside your projects folder.</p>}
              </div>
            )}
          </div>
          <small>Type any part of the path, then choose a suggestion—or enter the complete path directly.</small>
        </div>
        <button disabled={busy || !name.trim() || !workspace}>Add sleeping worker</button>
      </form>
      {available.length === 0 && <small className="privacy-note">Every discovered repository already has a worker. Advanced repository-root configuration will live in backup and installation settings.</small>}
    </section>
  );
}

type WorkerPreferenceRowProps = {
  worker: Worker;
  busy: boolean;
  first: boolean;
  last: boolean;
  dragging: boolean;
  onMove: (offset: -1 | 1) => void;
  onUpdate: Props["onUpdate"];
  onDragStart: () => void;
  onDragEnd: () => void;
  onDrop: (event: DragEvent) => void;
};

function WorkerPreferenceRow({ worker, busy, first, last, dragging, onMove, onUpdate, onDragStart, onDragEnd, onDrop }: WorkerPreferenceRowProps) {
  const [editing, setEditing] = useState(false);
  const [name, setName] = useState(worker.name);
  const [autostart, setAutostart] = useState(worker.autostart);

  async function save(event: FormEvent) {
    event.preventDefault();
    if (!name.trim()) return;
    await onUpdate(worker.id, name, autostart);
    setEditing(false);
  }

  function cancel() {
    setName(worker.name);
    setAutostart(worker.autostart);
    setEditing(false);
  }

  return (
    <div
      className={`configured-worker${dragging ? " configured-worker-dragging" : ""}`}
      draggable={!busy && !editing}
      onDragStart={(event) => { event.dataTransfer.effectAllowed = "move"; event.dataTransfer.setData("text/plain", worker.id); onDragStart(); }}
      onDragEnd={onDragEnd}
      onDragOver={(event) => { if (!editing) event.preventDefault(); }}
      onDrop={onDrop}
    >
      <span className="worker-settings-bee"><BeeMascot expression={worker.running ? "focused" : "sleeping"} /></span>
      {editing ? (
        <form className="worker-preference-form" aria-label={`Edit ${worker.name}`} onSubmit={(event) => void save(event)}>
          <label><span>Worker name</span><input value={name} onChange={(event) => setName(event.target.value)} maxLength={80} autoFocus /></label>
          <small>{repositoryName(worker.workspace)} · repository stays with this worker</small>
          <label className="worker-autostart"><input type="checkbox" checked={autostart} onChange={(event) => setAutostart(event.target.checked)} />Keep this worker active automatically</label>
          <span className="worker-edit-actions"><button disabled={busy || !name.trim()}>Save</button><button type="button" className="secondary-button" disabled={busy} onClick={cancel}>Cancel</button></span>
        </form>
      ) : (
        <>
          <span className="configured-worker-summary"><strong>{worker.name}</strong><small>{repositoryName(worker.workspace)} · {providerLabel(worker.provider)} · {worker.running ? "Buzzing" : "Sleeping"}{worker.autostart ? " · always active" : ""}</small></span>
          <button type="button" className="worker-edit-button secondary-button" disabled={busy} onClick={() => setEditing(true)}>Edit</button>
          <span className="worker-order-actions">
            <button type="button" className="secondary-button" aria-label={`Move ${worker.name} earlier`} disabled={busy || first} onClick={() => onMove(-1)}>↑</button>
            <button type="button" className="secondary-button" aria-label={`Move ${worker.name} later`} disabled={busy || last} onClick={() => onMove(1)}>↓</button>
          </span>
        </>
      )}
    </div>
  );
}

function repositoryName(workspace: string): string {
  return workspace.split(/[\\/]/).filter(Boolean).at(-1) ?? workspace;
}

function providerLabel(provider: ProviderKind): string {
  return provider === "codex" ? "Codex" : "Claude";
}

function workspaceMatches(choices: WorkspaceChoice[], query: string): WorkspaceChoice[] {
  const normalizedQuery = normalizePath(query.trim());
  if (!normalizedQuery) return choices;
  return choices
    .filter((choice) => normalizePath(choice.path).includes(normalizedQuery) || choice.name.toLowerCase().includes(normalizedQuery))
    .sort((left, right) => workspaceRank(left, normalizedQuery) - workspaceRank(right, normalizedQuery)
      || left.path.localeCompare(right.path));
}

function workspaceRank(choice: WorkspaceChoice, query: string): number {
  const path = normalizePath(choice.path);
  const name = choice.name.toLowerCase();
  if (path === query) return 0;
  if (name.startsWith(query)) return 1;
  if (path.startsWith(query)) return 2;
  return path.indexOf(query) + 3;
}

function normalizePath(path: string): string {
  return path.replaceAll("\\", "/").toLowerCase();
}

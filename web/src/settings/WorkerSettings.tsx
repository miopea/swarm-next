import { useState, type DragEvent, type FormEvent, type KeyboardEvent } from "react";

import type { ProviderCapabilities, ProviderKind, Worker, WorkspaceChoice } from "../api";
import BeeMascot from "../brand/BeeMascot";
import { useReorderDrag } from "../shared/useReorderDrag";
import { workerAttention } from "../workers/workerAttention";

type Props = {
  workers: Worker[];
  workspaces: WorkspaceChoice[];
  busy: boolean;
  providers: ProviderCapabilities;
  onCreate: (name: string, workspace: string, provider: ProviderKind, allowOutsideRoots: boolean) => Promise<void>;
  onUpdate: (workerId: string, name: string, description: string, provider: ProviderKind, autostart: boolean) => Promise<void>;
  onRemove: (workerId: string) => Promise<void>;
  onDraftDescription: (workerId: string) => Promise<string>;
  onImproveDescription?: (workerId: string) => Promise<string>;
  onReorder: (workerIds: string[]) => Promise<void>;
};

export default function WorkerSettings({ workers, workspaces, busy, providers, onCreate, onUpdate, onRemove, onDraftDescription, onImproveDescription, onReorder }: Props) {
  const scout = workers.find((worker) => worker.system_role === "scout");
  const roster = workers.filter((worker) => worker.role !== "queen" && worker.system_role !== "scout");
  const available = workspaces.filter((workspace) => !workspace.configured_worker_id);
  const [name, setName] = useState("");
  const [workspace, setWorkspace] = useState("");
  const [provider, setProvider] = useState<ProviderKind>("claude_code");
  const [workspaceOpen, setWorkspaceOpen] = useState(false);
  const [allowOutsideRoots, setAllowOutsideRoots] = useState(false);
  const [highlightedWorkspace, setHighlightedWorkspace] = useState(0);
  const workerReorder = useReorderDrag(roster.map((worker) => worker.id), (workerIds) => void onReorder(workerIds));
  const matchingWorkspaces = workspaceMatches(available, workspace).slice(0, 8);
  const customWorkspace = Boolean(workspace.trim()) && !workspaces.some((choice) => normalizePath(choice.path) === normalizePath(workspace.trim()));

  async function submit(event: FormEvent) {
    event.preventDefault();
    if (!name.trim() || !workspace || (customWorkspace && !allowOutsideRoots)) return;
    await onCreate(name, workspace, provider, customWorkspace && allowOutsideRoots);
    setName("");
    setWorkspace("");
    setAllowOutsideRoots(false);
    setWorkspaceOpen(false);
  }

  function selectWorkspace(choice: WorkspaceChoice) {
    setWorkspace(choice.path);
    setAllowOutsideRoots(false);
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
    workerReorder.dropBefore(targetWorkerId);
  }

  return (
    <section id="settings-crew" className="settings-card worker-settings" aria-labelledby="worker-settings-heading">
      <div><p className="eyebrow">Worker roster</p><h3 id="worker-settings-heading">Your familiar crew</h3></div>
      <p>Workers remember their repository and provider conversation across stops, updates, and reboots. Reorder them to match how you work.</p>
      <div className="configured-workers">
        {workers.find((worker) => worker.role === "queen") && (
          <div className="configured-worker queen-worker">
            <span className="worker-settings-bee"><BeeMascot expression="available" role="queen" /></span>
            <span><strong>Queen</strong><small>Pinned · always active</small></span>
          </div>
        )}
        {scout && (
          <WorkerPreferenceRow
            worker={scout}
            busy={busy}
            first
            last
            managed
            dragging={false}
            dropTarget={false}
            onMove={() => undefined}
            onUpdate={onUpdate}
            onRemove={onRemove}
            onDraftDescription={onDraftDescription}
            onImproveDescription={onImproveDescription}
            providers={providers}
            onDragStart={() => undefined}
            onDragEnd={() => undefined}
            onDragTarget={() => undefined}
            onDragLeave={() => undefined}
            onDrop={() => undefined}
          />
        )}
        {roster.map((worker, index) => (
          <WorkerPreferenceRow
            key={worker.id}
            worker={worker}
            busy={busy}
            first={index === 0}
            last={index === roster.length - 1}
            managed={false}
            dragging={workerReorder.draggedId === worker.id}
            dropTarget={workerReorder.dropTargetId === worker.id && workerReorder.draggedId !== worker.id}
            onMove={(offset) => move(index, offset)}
            onUpdate={onUpdate}
            onRemove={onRemove}
            onDraftDescription={onDraftDescription}
            onImproveDescription={onImproveDescription}
            providers={providers}
            onDragStart={() => workerReorder.start(worker.id)}
            onDragEnd={workerReorder.end}
            onDragTarget={() => workerReorder.target(worker.id)}
            onDragLeave={() => workerReorder.leave(worker.id)}
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
        </div>
        <div className="field-stack">
          <label htmlFor="configured-worker-repository">Repository</label>
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
              placeholder="Search by name or path"
              autoComplete="off"
              spellCheck={false}
              onFocus={() => setWorkspaceOpen(true)}
              onChange={(event) => {
                setWorkspace(event.target.value);
                setAllowOutsideRoots(false);
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
          <small>Start with a repository name and Swarm completes the path. Full paths still work.</small>
          {customWorkspace && <label className="outside-workspace-warning"><input type="checkbox" checked={allowOutsideRoots} onChange={(event) => setAllowOutsideRoots(event.target.checked)} /><span><strong>Use this path outside discovered project folders</strong><small>Only continue if you recognize and trust this folder. Swarm still requires an existing real directory and blocks files, symlinks, and filesystem roots.</small></span></label>}
        </div>
        <button disabled={busy || !name.trim() || !workspace || (customWorkspace && !allowOutsideRoots)}>Add sleeping worker</button>
      </form>
      <small className="privacy-note">New workers receive a private Queen-routing draft from local README and project metadata. Review or refresh it from Edit whenever the repository changes.</small>
      {available.length === 0 && <small className="privacy-note">Every discovered repository already has a worker. Advanced repository-root configuration will live in backup and installation settings.</small>}
    </section>
  );
}

type WorkerPreferenceRowProps = {
  worker: Worker;
  busy: boolean;
  first: boolean;
  last: boolean;
  managed: boolean;
  dragging: boolean;
  dropTarget: boolean;
  onMove: (offset: -1 | 1) => void;
  onUpdate: Props["onUpdate"];
  onRemove: Props["onRemove"];
  onDraftDescription: Props["onDraftDescription"];
  onImproveDescription: Props["onImproveDescription"];
  providers: ProviderCapabilities;
  onDragStart: () => void;
  onDragEnd: () => void;
  onDragTarget: () => void;
  onDragLeave: () => void;
  onDrop: (event: DragEvent) => void;
};

function WorkerPreferenceRow({ worker, busy, first, last, managed, dragging, dropTarget, onMove, onUpdate, onRemove, onDraftDescription, onImproveDescription, providers, onDragStart, onDragEnd, onDragTarget, onDragLeave, onDrop }: WorkerPreferenceRowProps) {
  const [editing, setEditing] = useState(false);
  const [name, setName] = useState(worker.name);
  const [description, setDescription] = useState(worker.description ?? "");
  const [provider, setProvider] = useState(worker.provider);
  const [autostart, setAutostart] = useState(worker.autostart);
  const [confirmingRemoval, setConfirmingRemoval] = useState(false);
  const [draftingDescription, setDraftingDescription] = useState(false);
  const [draftError, setDraftError] = useState("");
  const [improvingDescription, setImprovingDescription] = useState(false);
  const attention = workerAttention(worker);

  async function save(event: FormEvent) {
    event.preventDefault();
    if (!name.trim()) return;
    await onUpdate(worker.id, name, description, provider, autostart);
    setEditing(false);
  }

  function cancel() {
    setName(worker.name);
    setDescription(worker.description ?? "");
    setProvider(worker.provider);
    setAutostart(worker.autostart);
    setConfirmingRemoval(false);
    setEditing(false);
  }

  async function remove() {
    await onRemove(worker.id);
  }

  async function draftDescription() {
    setDraftingDescription(true);
    setDraftError("");
    try {
      setDescription(await onDraftDescription(worker.id));
    } catch {
      setDraftError("Swarm could not draft from this repository. You can still enter the description yourself.");
    } finally {
      setDraftingDescription(false);
    }
  }

  async function improveDescription() {
    if (!onImproveDescription) return;
    setImprovingDescription(true);
    setDraftError("");
    try {
      setDescription(await onImproveDescription(worker.id));
    } catch {
      setDraftError("Claude could not improve this description. The current editable draft is unchanged.");
    } finally {
      setImprovingDescription(false);
    }
  }

  return (
    <div
      className={`configured-worker${dragging ? " configured-worker-dragging" : ""}${dropTarget ? " drop-target-before" : ""}`}
      draggable={!managed && !busy && !editing}
      onDragStart={(event) => { event.dataTransfer.effectAllowed = "move"; event.dataTransfer.setData("text/plain", worker.id); onDragStart(); }}
      onDragEnd={onDragEnd}
      onDragEnter={() => { if (!editing) onDragTarget(); }}
      onDragOver={(event) => { if (!editing) { event.preventDefault(); onDragTarget(); } }}
      onDragLeave={(event) => { if (!event.currentTarget.contains(event.relatedTarget as Node | null)) onDragLeave(); }}
      onDrop={onDrop}
    >
      <span className="worker-settings-bee"><BeeMascot expression={attention.expression} /></span>
      {editing ? (
        <form className="worker-preference-form" aria-label={`Edit ${worker.name}`} onSubmit={(event) => void save(event)}>
          <label><span>Worker name</span><input value={name} disabled={managed} onChange={(event) => setName(event.target.value)} maxLength={80} autoFocus /></label>
          {managed && <small className="privacy-note">Scout is pinned after Queen and keeps ordinary worker authority for deliberate cross-repository work.</small>}
          <small className="worker-repository-path"><span>Repository</span><code>{worker.workspace}</code></small>
          <div className="worker-description-field"><span className="worker-description-heading"><label htmlFor={`worker-description-${worker.id}`}>Queen routing description</label><span className="worker-description-actions"><button type="button" className="secondary-button" disabled={busy || draftingDescription || improvingDescription} onClick={() => void draftDescription()}>{draftingDescription ? "Reading repository…" : description ? "Refresh local draft" : "Draft locally"}</button>{onImproveDescription && <button type="button" className="secondary-button" disabled={busy || draftingDescription || improvingDescription} onClick={() => void improveDescription()}>{improvingDescription ? "Claude is reviewing…" : "Improve with Claude"}</button>}</span></span><textarea id={`worker-description-${worker.id}`} value={description} onChange={(event) => setDescription(event.target.value)} maxLength={2000} rows={3} placeholder="What this repository owns and when Queen should route work here" /><small>Local drafting reads bounded README and manifest metadata. Improve with Claude sends only that metadata in one tool-free turn (up to $0.10). Nothing is saved until you choose Save.</small>{draftError && <small className="field-error" role="alert">{draftError}</small>}</div>
          <div className="worker-provider-field"><label htmlFor={`worker-provider-${worker.id}`}>Default coding provider</label><select id={`worker-provider-${worker.id}`} value={provider} disabled={worker.running} onChange={(event) => setProvider(event.target.value as ProviderKind)}>
            <option value="claude_code" disabled={!providers.claude_code}>Claude Code{providers.claude_code ? "" : " · unavailable"}</option>
            <option value="codex" disabled={!providers.codex}>Codex{providers.codex ? "" : " · unavailable"}</option>
          </select><small>{worker.running ? "Put this worker to sleep before changing provider." : "Used the next time this worker wakes. Existing history remains available."}</small></div>
          <label className="worker-autostart"><input type="checkbox" checked={autostart} onChange={(event) => setAutostart(event.target.checked)} />Keep this worker active automatically</label>
          <span className="worker-edit-actions"><button disabled={busy || !name.trim()}>Save</button><button type="button" className="secondary-button" disabled={busy} onClick={cancel}>Cancel</button></span>
          {!managed && <div className="worker-remove-zone">
            {confirmingRemoval ? <><p><strong>Remove {worker.name} from this Hive?</strong><small>Repository files are untouched. Historical sessions remain, but this worker must be sleeping and have no open assigned tasks.</small></p><span><button type="button" className="danger-button" disabled={busy || worker.running} onClick={() => void remove()}>Confirm removal</button><button type="button" className="secondary-button" disabled={busy} onClick={() => setConfirmingRemoval(false)}>Keep worker</button></span></> : <button type="button" className="danger-link" disabled={busy || worker.running} onClick={() => setConfirmingRemoval(true)}>Remove worker</button>}
          </div>}
        </form>
      ) : (
        <>
          <span className="configured-worker-summary"><strong>{worker.name}</strong><small>{repositoryName(worker.workspace)} · {providerLabel(worker.provider)} · {attention.label}{worker.autostart ? " · always active" : ""}</small>{worker.description && <small className="worker-routing-summary">{worker.description}</small>}</span>
          <button type="button" className="worker-edit-button secondary-button" disabled={busy} onClick={() => setEditing(true)}>Edit</button>
          {!managed && <span className="worker-order-actions">
            <button type="button" className="secondary-button" aria-label={`Move ${worker.name} earlier`} disabled={busy || first} onClick={() => onMove(-1)}>↑</button>
            <button type="button" className="secondary-button" aria-label={`Move ${worker.name} later`} disabled={busy || last} onClick={() => onMove(1)}>↓</button>
          </span>}
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

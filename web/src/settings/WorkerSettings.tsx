import { useState, type DragEvent, type FormEvent } from "react";

import type { Worker, WorkspaceChoice } from "../api";
import BeeMascot from "../brand/BeeMascot";

type Props = {
  workers: Worker[];
  workspaces: WorkspaceChoice[];
  busy: boolean;
  onCreate: (name: string, workspace: string) => Promise<void>;
  onUpdate: (workerId: string, name: string, autostart: boolean) => Promise<void>;
  onReorder: (workerIds: string[]) => Promise<void>;
};

export default function WorkerSettings({ workers, workspaces, busy, onCreate, onUpdate, onReorder }: Props) {
  const roster = workers.filter((worker) => worker.role !== "queen");
  const available = workspaces.filter((workspace) => !workspace.configured_worker_id);
  const [name, setName] = useState("");
  const [workspace, setWorkspace] = useState("");
  const [draggedWorkerId, setDraggedWorkerId] = useState<string>();

  async function submit(event: FormEvent) {
    event.preventDefault();
    if (!name.trim() || !workspace) return;
    await onCreate(name, workspace);
    setName("");
    setWorkspace("");
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
      <p>Workers remember their repository and Claude conversation across stops, updates, and reboots. Reorder them to match how you work.</p>
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
        <div className="field-stack">
          <label htmlFor="configured-worker-repository">Repository</label>
          <select id="configured-worker-repository" value={workspace} onChange={(event) => setWorkspace(event.target.value)}>
            <option value="">Choose a repository</option>
            {available.map((choice) => <option key={choice.path} value={choice.path}>{choice.name}{choice.kind === "folder" ? " · folder" : ""}</option>)}
          </select>
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
          <span className="configured-worker-summary"><strong>{worker.name}</strong><small>{repositoryName(worker.workspace)} · {worker.running ? "Buzzing" : "Sleeping"}{worker.autostart ? " · always active" : ""}</small></span>
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

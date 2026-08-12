import { useState, type FormEvent } from "react";

import type { Worker, WorkspaceChoice } from "../api";
import BeeMascot from "../brand/BeeMascot";

type Props = {
  workers: Worker[];
  workspaces: WorkspaceChoice[];
  busy: boolean;
  onCreate: (name: string, workspace: string) => Promise<void>;
  onReorder: (workerIds: string[]) => Promise<void>;
};

export default function WorkerSettings({ workers, workspaces, busy, onCreate, onReorder }: Props) {
  const roster = workers.filter((worker) => worker.role !== "queen");
  const available = workspaces.filter((workspace) => !workspace.configured_worker_id);
  const [name, setName] = useState("");
  const [workspace, setWorkspace] = useState("");

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
          <div className="configured-worker" key={worker.id}>
            <span className="worker-settings-bee"><BeeMascot expression={worker.running ? "focused" : "sleeping"} /></span>
            <span><strong>{worker.name}</strong><small>{repositoryName(worker.workspace)} · {worker.running ? "Buzzing" : "Sleeping"}</small></span>
            <span className="worker-order-actions">
              <button type="button" aria-label={`Move ${worker.name} earlier`} disabled={busy || index === 0} onClick={() => move(index, -1)}>↑</button>
              <button type="button" aria-label={`Move ${worker.name} later`} disabled={busy || index === roster.length - 1} onClick={() => move(index, 1)}>↓</button>
            </span>
          </div>
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

function repositoryName(workspace: string): string {
  return workspace.split(/[\\/]/).filter(Boolean).at(-1) ?? workspace;
}

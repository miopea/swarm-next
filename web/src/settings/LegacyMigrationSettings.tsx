import { useEffect, useState } from "react";

import {
  commitLegacyTaskMigration,
  commitLegacyWorkerMigration,
  discoverLocalLegacyMigration,
  listActiveLegacyTaskMigrations,
  listActiveLegacyWorkerMigrations,
  previewLegacyTaskMigration,
  previewLegacyWorkerMigration,
  rollbackLegacyTaskMigration,
  rollbackLegacyWorkerMigration,
  type LegacyMigrationBundle,
  type LegacyMigrationPreview,
  type LegacyMigrationReceipt,
  type LegacyTaskPreview,
  type LegacyWorkerMigrationPreview,
  type LegacyWorkerMigrationReceipt,
  type LegacyWorkerPreview,
} from "../api/migration";
import { downloadJson } from "../shared/download";

type Props = {
  busy: boolean;
  operatorToken: string;
};

const MAX_FILE_BYTES = 16 * 1024 * 1024;

export default function LegacyMigrationSettings({ busy, operatorToken }: Props) {
  const [bundle, setBundle] = useState<LegacyMigrationBundle>();
  const [preview, setPreview] = useState<LegacyMigrationPreview>();
  const [workerPreview, setWorkerPreview] = useState<LegacyWorkerMigrationPreview>();
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [selectedWorkers, setSelectedWorkers] = useState<Set<string>>(new Set());
  const [receipt, setReceipt] = useState<LegacyMigrationReceipt>();
  const [workerReceipt, setWorkerReceipt] = useState<LegacyWorkerMigrationReceipt>();
  const [working, setWorking] = useState(false);
  const [confirmImport, setConfirmImport] = useState(false);
  const [confirmRollback, setConfirmRollback] = useState(false);
  const [confirmWorkerImport, setConfirmWorkerImport] = useState(false);
  const [confirmWorkerRollback, setConfirmWorkerRollback] = useState(false);
  const [message, setMessage] = useState<string>();
  const disabled = busy || working;

  useEffect(() => {
    let cancelled = false;
    void listActiveLegacyTaskMigrations(operatorToken)
      .then((receipts) => {
        if (!cancelled && receipts.length > 0) setReceipt(receipts[0]);
      })
      .catch(() => {
        // Import remains available if older daemons do not expose receipt recovery yet.
      });
    void listActiveLegacyWorkerMigrations(operatorToken)
      .then((receipts) => {
        if (!cancelled && receipts.length > 0) setWorkerReceipt(receipts[0]);
      })
      .catch(() => {
        // Older daemons may not have the worker migration slice yet.
      });
    return () => { cancelled = true; };
  }, [operatorToken]);

  async function previewBundle(parsed: LegacyMigrationBundle) {
    const [nextPreview, nextWorkerPreview] = await Promise.all([
      previewLegacyTaskMigration(operatorToken, parsed),
      previewLegacyWorkerMigration(operatorToken, parsed),
    ]);
    setBundle(parsed);
    setPreview(nextPreview);
    setWorkerPreview(nextWorkerPreview);
    setSelected(new Set(nextPreview.records.filter((record) => record.selectable).map((record) => record.source_id)));
    setSelectedWorkers(new Set(nextWorkerPreview.records.filter((record) => record.selectable).map((record) => record.source_id)));
    setMessage(nextPreview.selectable === 0 && nextWorkerPreview.selectable === 0 ? "Nothing in this Legacy Hive is ready to import." : undefined);
  }

  async function findLocalHive() {
    setMessage(undefined);
    setReceipt(undefined);
    setConfirmImport(false);
    setWorking(true);
    try {
      await previewBundle(await discoverLocalLegacyMigration(operatorToken));
    } catch {
      setBundle(undefined);
      setPreview(undefined);
      setWorkerPreview(undefined);
      setSelected(new Set());
      setSelectedWorkers(new Set());
      setMessage("Swarm could not find a compatible Legacy Hive on this machine. Use the advanced file option only if Legacy was installed somewhere unusual.");
    } finally {
      setWorking(false);
    }
  }

  async function choosePackage(file: File | undefined) {
    if (!file) return;
    setMessage(undefined);
    setReceipt(undefined);
    setConfirmImport(false);
    if (file.size > MAX_FILE_BYTES) {
      setMessage("That migration package is larger than the 16 MB safety limit.");
      return;
    }
    setWorking(true);
    try {
      await previewBundle(JSON.parse(await file.text()) as LegacyMigrationBundle);
    } catch {
      setBundle(undefined);
      setPreview(undefined);
      setWorkerPreview(undefined);
      setSelected(new Set());
      setSelectedWorkers(new Set());
      setMessage("Swarm could not read this migration package. Create a fresh package and try again.");
    } finally {
      setWorking(false);
    }
  }

  function toggleRecord(record: LegacyTaskPreview) {
    if (!record.selectable) return;
    setSelected((current) => {
      const next = new Set(current);
      if (next.has(record.source_id)) next.delete(record.source_id);
      else next.add(record.source_id);
      return next;
    });
    setConfirmImport(false);
  }

  function toggleWorker(record: LegacyWorkerPreview) {
    if (!record.selectable) return;
    setSelectedWorkers((current) => {
      const next = new Set(current);
      if (next.has(record.source_id)) next.delete(record.source_id);
      else next.add(record.source_id);
      return next;
    });
    setConfirmWorkerImport(false);
  }

  async function importSelectedWorkers() {
    if (!bundle || !workerPreview || selectedWorkers.size === 0) return;
    setWorking(true);
    setMessage(undefined);
    try {
      const nextReceipt = await commitLegacyWorkerMigration(
        operatorToken,
        bundle,
        workerPreview,
        [...selectedWorkers],
      );
      setWorkerReceipt(nextReceipt);
      setConfirmWorkerImport(false);
      try {
        const refreshedTaskPreview = await previewLegacyTaskMigration(operatorToken, bundle);
        setPreview(refreshedTaskPreview);
        setSelected(new Set(refreshedTaskPreview.records.filter((record) => record.selectable).map((record) => record.source_id)));
        setMessage(`${nextReceipt.imported_worker_ids.length} sleeping worker${nextReceipt.imported_worker_ids.length === 1 ? " was" : "s were"} added. Task matches were refreshed; no provider process was started.`);
      } catch {
        setMessage(`${nextReceipt.imported_worker_ids.length} sleeping worker${nextReceipt.imported_worker_ids.length === 1 ? " was" : "s were"} added. Reopen this package before importing tasks so worker matches are current.`);
      }
    } catch {
      setMessage("The worker import was not applied. Refresh the preview before trying again.");
    } finally {
      setWorking(false);
    }
  }

  async function rollbackWorkers() {
    if (!workerReceipt) return;
    setWorking(true);
    setMessage(undefined);
    try {
      const result = await rollbackLegacyWorkerMigration(operatorToken, workerReceipt);
      setWorkerReceipt(undefined);
      setConfirmWorkerRollback(false);
      setMessage(`${result.removed_workers} untouched imported worker${result.removed_workers === 1 ? " was" : "s were"} removed.`);
    } catch {
      setConfirmWorkerRollback(false);
      setMessage("An imported worker has changed or been used, so Swarm protected it from rollback.");
    } finally {
      setWorking(false);
    }
  }

  async function importSelected() {
    if (!bundle || !preview || selected.size === 0) return;
    setWorking(true);
    setMessage(undefined);
    try {
      const nextReceipt = await commitLegacyTaskMigration(operatorToken, bundle, preview, [...selected]);
      setReceipt(nextReceipt);
      setConfirmImport(false);
      setMessage(`${nextReceipt.imported_task_ids.length} task${nextReceipt.imported_task_ids.length === 1 ? "" : "s"} staged for review. No workers were started.`);
    } catch {
      setMessage("The import was not applied. Refresh the preview before trying again.");
    } finally {
      setWorking(false);
    }
  }

  async function rollback() {
    if (!receipt) return;
    setWorking(true);
    setMessage(undefined);
    try {
      const result = await rollbackLegacyTaskMigration(operatorToken, receipt);
      setReceipt(undefined);
      setBundle(undefined);
      setPreview(undefined);
      setSelected(new Set());
      setConfirmRollback(false);
      setMessage(`${result.removed_tasks} untouched imported task${result.removed_tasks === 1 ? " was" : "s were"} removed.`);
    } catch {
      setConfirmRollback(false);
      setMessage("This batch has already changed, so Swarm protected it from rollback.");
    } finally {
      setWorking(false);
    }
  }

  return (
    <section id="settings-migration" className="settings-card migration-settings" aria-labelledby="migration-heading">
      <div><p className="eyebrow">Migration</p><h3 id="migration-heading">Bring your Legacy Hive forward</h3></div>
      <p>Preview the familiar crew and open local work before anything changes. Workers arrive sleeping; tasks remain Draft until you approve them. Jira work stays in Jira.</p>

      {!preview && !workerPreview && !receipt && !workerReceipt && (
        <div className="migration-discovery">
          <button type="button" className="primary-action" disabled={disabled} onClick={() => void findLocalHive()}>
            {working ? "Finding your Legacy Hive…" : "Find my Legacy Hive"}
          </button>
          <span>Swarm looks in the normal Legacy location on this computer, reads it safely, and shows a preview. Nothing is moved yet.</span>
          <details>
            <summary>Legacy was installed somewhere else</summary>
            <label className={`migration-drop ${disabled ? "disabled" : ""}`}>
              <strong>Use a migration file</strong>
              <span>Choose an exported package only for a custom or remote Legacy install.</span>
              <input
                type="file"
                accept="application/json,.json"
                disabled={disabled}
                onChange={(event) => void choosePackage(event.target.files?.[0])}
              />
            </label>
          </details>
        </div>
      )}

      {workerPreview && !workerReceipt && workerPreview.records.length > 0 && (
        <div className="migration-section" aria-labelledby="migration-workers-heading">
          <div><p className="eyebrow">Crew</p><h4 id="migration-workers-heading">Review familiar workers</h4></div>
          <p>Repository paths and reviewed descriptions come across. Workers stay sleeping, and provider conversations, identity files, groups, and approval rules stay behind.</p>
          <div className="migration-summary" aria-label="Worker migration preview summary">
            <span><strong>{workerPreview.selectable}</strong><small>Ready to add</small></span>
            <span><strong>{workerPreview.skipped}</strong><small>Already represented</small></span>
            <span><strong>{workerPreview.invalid}</strong><small>Needs attention</small></span>
          </div>
          <div className="migration-selection-actions">
            <button type="button" className="text-button" onClick={() => setSelectedWorkers(new Set(workerPreview.records.filter((record) => record.selectable).map((record) => record.source_id)))}>Select recommended</button>
            <button type="button" className="text-button" onClick={() => setSelectedWorkers(new Set())}>Clear</button>
            {!preview?.records.length && <button type="button" className="text-button" onClick={() => { setBundle(undefined); setPreview(undefined); setWorkerPreview(undefined); setSelectedWorkers(new Set()); }}>Start over</button>}
          </div>
          <div className="migration-records" role="list" aria-label="Legacy worker migration preview">
            {workerPreview.records.map((record) => (
              <label key={record.source_id} className={`migration-record ${record.selectable ? "" : "unavailable"}`}>
                <input
                  type="checkbox"
                  checked={selectedWorkers.has(record.source_id)}
                  disabled={!record.selectable || disabled}
                  onChange={() => toggleWorker(record)}
                />
                <span className="migration-record-copy">
                  <strong>{record.name || "Unnamed Legacy worker"}</strong>
                  <small>{record.workspace} · {record.provider === "codex" ? "Codex" : "Claude Code"} · Sleeping</small>
                  {record.warnings.map((warning) => <small key={warning} className="migration-warning">{warning}</small>)}
                </span>
              </label>
            ))}
          </div>
          {!confirmWorkerImport ? (
            <button type="button" className="primary-action" disabled={disabled || selectedWorkers.size === 0} onClick={() => setConfirmWorkerImport(true)}>Review {selectedWorkers.size} worker{selectedWorkers.size === 1 ? "" : "s"}</button>
          ) : (
            <div className="migration-confirmation" role="group" aria-label="Confirm Legacy worker import">
              <strong>Add {selectedWorkers.size} sleeping worker{selectedWorkers.size === 1 ? "" : "s"}?</strong>
              <span>Swarm creates durable roster entries only. No Claude or Codex process starts.</span>
              <div className="settings-actions">
                <button type="button" className="secondary-button" disabled={disabled} onClick={() => setConfirmWorkerImport(false)}>Keep reviewing</button>
                <button type="button" className="primary-action" disabled={disabled} onClick={() => void importSelectedWorkers()}>{working ? "Adding…" : "Add selected workers"}</button>
              </div>
            </div>
          )}
        </div>
      )}

      {workerReceipt && (
        <div className="migration-receipt" role="status">
          <strong>Familiar crew added safely</strong>
          <p>{workerReceipt.imported_worker_ids.length} worker{workerReceipt.imported_worker_ids.length === 1 ? " is" : "s are"} sleeping in the roster. Review names, repositories, providers, and descriptions before waking anyone.</p>
          {!confirmWorkerRollback ? (
            <button type="button" className="secondary-button" disabled={disabled} onClick={() => setConfirmWorkerRollback(true)}>Undo untouched worker import</button>
          ) : (
            <div className="migration-confirmation">
              <strong>Remove these imported workers?</strong>
              <span>Swarm refuses if any worker was edited, awakened, or assigned work.</span>
              <div className="settings-actions">
                <button type="button" className="secondary-button" disabled={disabled} onClick={() => setConfirmWorkerRollback(false)}>Keep workers</button>
                <button type="button" className="danger-button" disabled={disabled} onClick={() => void rollbackWorkers()}>{working ? "Checking…" : "Undo worker import"}</button>
              </div>
            </div>
          )}
        </div>
      )}

      {preview && !receipt && (
        <>
          <div className="migration-summary" aria-label="Migration preview summary">
            <span><strong>{preview.selectable}</strong><small>Ready to review</small></span>
            <span><strong>{preview.skipped}</strong><small>Left in its source</small></span>
            <span><strong>{preview.invalid}</strong><small>Needs attention</small></span>
          </div>
          <div className="migration-selection-actions">
            <button type="button" className="text-button" onClick={() => setSelected(new Set(preview.records.filter((record) => record.selectable).map((record) => record.source_id)))}>Select recommended</button>
            <button type="button" className="text-button" onClick={() => setSelected(new Set())}>Clear</button>
            <button type="button" className="text-button" onClick={() => { setBundle(undefined); setPreview(undefined); setWorkerPreview(undefined); setSelected(new Set()); setSelectedWorkers(new Set()); }}>Start over</button>
          </div>
          <div className="migration-records" role="list" aria-label="Legacy task migration preview">
            {preview.records.map((record) => (
              <label key={record.source_id} className={`migration-record ${record.selectable ? "" : "unavailable"}`}>
                <input
                  type="checkbox"
                  checked={selected.has(record.source_id)}
                  disabled={!record.selectable || disabled}
                  onChange={() => toggleRecord(record)}
                />
                <span className="migration-record-copy">
                  <strong>{record.title || "Untitled Legacy task"}</strong>
                  <small>{migrationRecordSummary(record)}</small>
                  {record.warnings.map((warning) => <small key={warning} className="migration-warning">{warning}</small>)}
                </span>
              </label>
            ))}
          </div>
          {!confirmImport ? (
            <button type="button" className="primary-action" disabled={disabled || selected.size === 0} onClick={() => setConfirmImport(true)}>Review import of {selected.size}</button>
          ) : (
            <div className="migration-confirmation" role="group" aria-label="Confirm Legacy task import">
              <strong>Import {selected.size} selected task{selected.size === 1 ? "" : "s"}?</strong>
              <span>They will appear as Drafts on the task board. Existing workers stay asleep and Legacy is not changed.</span>
              <div className="settings-actions">
                <button type="button" className="secondary-button" disabled={disabled} onClick={() => setConfirmImport(false)}>Keep reviewing</button>
                <button type="button" className="primary-action" disabled={disabled} onClick={() => void importSelected()}>{working ? "Importing…" : "Import selected tasks"}</button>
              </div>
            </div>
          )}
        </>
      )}

      {receipt && (
        <div className="migration-receipt" role="status">
          <strong>Imported safely</strong>
          <p>Review these Drafts on the task board before approving normal work or finishing the handoff from Legacy. They remain visible and actionable in Legacy.</p>
          <button type="button" className="secondary-button" onClick={() => downloadJson(receipt, `swarm-next-migration-receipt-${receipt.batch_id}.json`)}>Download migration receipt</button>
          {!confirmRollback ? (
            <button type="button" className="secondary-button" disabled={disabled} onClick={() => setConfirmRollback(true)}>Undo this untouched import</button>
          ) : (
            <div className="migration-confirmation">
              <strong>Remove this import batch?</strong>
              <span>Swarm will refuse if any imported task has been changed.</span>
              <div className="settings-actions">
                <button type="button" className="secondary-button" disabled={disabled} onClick={() => setConfirmRollback(false)}>Keep tasks</button>
                <button type="button" className="danger-button" disabled={disabled} onClick={() => void rollback()}>{working ? "Checking…" : "Undo import"}</button>
              </div>
            </div>
          )}
        </div>
      )}
      {message && <p className="migration-message" role="status">{message}</p>}
      <small className="privacy-note">Migration reads only the Legacy crew and open local tasks needed for this preview. Credentials, terminal sessions, provider conversations, identity files, and Jira records are never included.</small>
    </section>
  );
}

function migrationRecordSummary(record: LegacyTaskPreview) {
  if (record.disposition === "skipped_jira") return "Jira issue · returns through Jira sync";
  if (record.disposition === "skipped_closed") return "Closed in Legacy · remains in Legacy history";
  if (record.disposition === "duplicate") return "Already represented in Swarm Next";
  if (record.disposition === "invalid") return "Cannot import until its source data is repaired";
  const worker = record.matched_worker_name ? ` · ${record.matched_worker_name}` : " · Unassigned";
  return `${record.source_status} → ${record.target_state}${worker}`;
}

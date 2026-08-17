import { useEffect, useState } from "react";

import {
  commitLegacyTaskMigration,
  listActiveLegacyTaskMigrations,
  previewLegacyTaskMigration,
  rollbackLegacyTaskMigration,
  type LegacyMigrationBundle,
  type LegacyMigrationPreview,
  type LegacyMigrationReceipt,
  type LegacyTaskPreview,
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
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [receipt, setReceipt] = useState<LegacyMigrationReceipt>();
  const [working, setWorking] = useState(false);
  const [confirmImport, setConfirmImport] = useState(false);
  const [confirmRollback, setConfirmRollback] = useState(false);
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
    return () => { cancelled = true; };
  }, [operatorToken]);

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
      const parsed = JSON.parse(await file.text()) as LegacyMigrationBundle;
      const nextPreview = await previewLegacyTaskMigration(operatorToken, parsed);
      setBundle(parsed);
      setPreview(nextPreview);
      setSelected(new Set(nextPreview.records.filter((record) => record.selectable).map((record) => record.source_id)));
      setMessage(nextPreview.selectable === 0 ? "Nothing in this package is ready to import." : undefined);
    } catch {
      setBundle(undefined);
      setPreview(undefined);
      setSelected(new Set());
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
      <div><p className="eyebrow">Migration</p><h3 id="migration-heading">Bring your open Legacy work with you</h3></div>
      <p>Preview a package before anything changes. Jira work stays in Jira, completed history stays in Legacy, and imports remain Draft until you approve them on the task board.</p>

      {!preview && !receipt && (
        <label className={`migration-drop ${disabled ? "disabled" : ""}`}>
          <strong>{working ? "Checking package…" : "Choose Legacy migration package"}</strong>
          <span>Swarm validates every record and shows exactly what would change.</span>
          <input
            type="file"
            accept="application/json,.json"
            disabled={disabled}
            onChange={(event) => void choosePackage(event.target.files?.[0])}
          />
        </label>
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
            <button type="button" className="text-button" onClick={() => { setBundle(undefined); setPreview(undefined); setSelected(new Set()); }}>Choose another package</button>
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
      <small className="privacy-note">Migration packages contain private task content. Store them like a Hive backup. Repositories, credentials, terminal sessions, and Jira records are never included.</small>
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

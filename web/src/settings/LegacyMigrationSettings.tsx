import { useEffect, useState } from "react";

import { RuntimeRequestError } from "../api";
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
import { useModalFocus } from "../shared/useModalFocus";

type Props = {
  busy: boolean;
  operatorToken: string;
  onOpenTasks?: () => void;
};

const MAX_FILE_BYTES = 16 * 1024 * 1024;

export default function LegacyMigrationSettings({ busy, operatorToken, onOpenTasks = () => window.location.assign("?surface=tasks") }: Props) {
  const [bundle, setBundle] = useState<LegacyMigrationBundle>();
  const [preview, setPreview] = useState<LegacyMigrationPreview>();
  const [workerPreview, setWorkerPreview] = useState<LegacyWorkerMigrationPreview>();
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [selectedWorkers, setSelectedWorkers] = useState<Set<string>>(new Set());
  const [resumeLegacyConversations, setResumeLegacyConversations] = useState(true);
  const [replaceExistingConversations, setReplaceExistingConversations] = useState(false);
  const [receipt, setReceipt] = useState<LegacyMigrationReceipt>();
  const [workerReceipt, setWorkerReceipt] = useState<LegacyWorkerMigrationReceipt>();
  const [working, setWorking] = useState(false);
  const [confirmImport, setConfirmImport] = useState(false);
  const [confirmRollback, setConfirmRollback] = useState(false);
  const [confirmWorkerImport, setConfirmWorkerImport] = useState(false);
  const [confirmWorkerRollback, setConfirmWorkerRollback] = useState(false);
  const [showClosedHistory, setShowClosedHistory] = useState(false);
  const [showInvalidRecords, setShowInvalidRecords] = useState(false);
  const [showExcludedRecords, setShowExcludedRecords] = useState(false);
  const [message, setMessage] = useState<string>();
  const [receiptRecoveryUnavailable, setReceiptRecoveryUnavailable] = useState(false);
  const [receiptRecoveryAttempt, setReceiptRecoveryAttempt] = useState(0);
  const disabled = busy || working;
  const closedHistoryCount = preview?.records.filter((record) => record.disposition === "skipped_closed").length ?? 0;
  const invalidRecordCount = preview?.records.filter((record) => record.disposition === "invalid").length ?? 0;
  const excludedRecordCount = preview?.records.filter((record) => !record.selectable && record.disposition !== "invalid" && record.disposition !== "skipped_closed").length ?? 0;
  const visibleTaskRecords = preview?.records.filter((record) => (
    record.selectable
    || (record.disposition === "invalid" && showInvalidRecords)
    || (record.disposition === "skipped_closed" && showClosedHistory)
    || (!record.selectable && record.disposition !== "invalid" && record.disposition !== "skipped_closed" && showExcludedRecords)
  )) ?? [];
  const migrationComplete = Boolean(receipt && workerReceipt);

  useEffect(() => {
    let cancelled = false;
    void Promise.allSettled([
      listActiveLegacyTaskMigrations(operatorToken),
      listActiveLegacyWorkerMigrations(operatorToken),
    ]).then(([tasks, workers]) => {
      if (cancelled) return;
      if (tasks.status === "fulfilled" && tasks.value.length > 0) setReceipt(tasks.value[0]);
      if (workers.status === "fulfilled" && workers.value.length > 0) setWorkerReceipt(workers.value[0]);
      setReceiptRecoveryUnavailable(
        (tasks.status === "rejected" && !isMissingReceiptEndpoint(tasks.reason))
        || (workers.status === "rejected" && !isMissingReceiptEndpoint(workers.reason)),
      );
      });
    return () => { cancelled = true; };
  }, [operatorToken, receiptRecoveryAttempt]);

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
    if (!canSelectWorker(record, replaceExistingConversations)) return;
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
        resumeLegacyConversations,
        replaceExistingConversations,
      );
      setWorkerReceipt(nextReceipt);
      setConfirmWorkerImport(false);
      try {
        const refreshedTaskPreview = await previewLegacyTaskMigration(operatorToken, bundle);
        setPreview(refreshedTaskPreview);
        setSelected(new Set(refreshedTaskPreview.records.filter((record) => record.selectable).map((record) => record.source_id)));
        const resumedCount = nextReceipt.resumed_source_ids?.length ?? 0;
        const addedCount = nextReceipt.imported_worker_ids.length;
        const updatedCount = nextReceipt.updated_worker_ids?.length ?? 0;
        setMessage(`${addedCount} sleeping worker${addedCount === 1 ? " was" : "s were"} added and ${updatedCount} matching worker conversation${updatedCount === 1 ? " was" : "s were"} replaced. ${resumedCount} will resume ${resumedCount === 1 ? "its" : "their"} exact Legacy conversation when first awakened. Task matches were refreshed; no provider process was started.`);
      } catch {
        setMessage(`${nextReceipt.imported_worker_ids.length} sleeping worker${nextReceipt.imported_worker_ids.length === 1 ? " was" : "s were"} added. Reopen this package before importing tasks so worker matches are current.`);
      }
    } catch {
      setMessage(replaceExistingConversations
        ? "The worker migration was not applied. Put every selected matching worker to sleep, refresh the preview, and try again. No conversation was changed."
        : "The worker import was not applied. Refresh the preview before trying again.");
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
      setMessage(`${result.removed_workers} untouched imported worker${result.removed_workers === 1 ? " was" : "s were"} removed and ${result.restored_workers} matching worker conversation${result.restored_workers === 1 ? " was" : "s were"} restored.`);
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
      {receiptRecoveryUnavailable ? <div className="form-error migration-recovery-error" role="alert"><span>Swarm could not verify whether an earlier import can still be undone. Existing workers and tasks are unchanged.</span><button className="secondary-button" type="button" onClick={() => setReceiptRecoveryAttempt((attempt) => attempt + 1)}>Retry migration history</button></div> : null}

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
          <p>Repository paths and reviewed descriptions come across. Workers stay sleeping. You can also preserve exact Claude or Codex conversations found for their repositories; identity files, groups, and approval rules stay behind.</p>
          <div className="migration-summary" aria-label="Worker migration preview summary">
            <span><strong>{workerPreview.selectable}</strong><small>Ready to add</small></span>
            <span><strong>{workerPreview.skipped}</strong><small>Already represented</small></span>
            <span><strong>{workerPreview.invalid}</strong><small>Needs attention</small></span>
          </div>
          <div className="migration-selection-actions">
            <button type="button" className="text-button" onClick={() => setSelectedWorkers(new Set(workerPreview.records.filter((record) => canSelectWorker(record, replaceExistingConversations)).map((record) => record.source_id)))}>Select recommended</button>
            <button type="button" className="text-button" onClick={() => setSelectedWorkers(new Set())}>Clear</button>
            {!preview?.records.length && <button type="button" className="text-button" onClick={() => { setBundle(undefined); setPreview(undefined); setWorkerPreview(undefined); setSelectedWorkers(new Set()); }}>Start over</button>}
          </div>
          <div className="migration-records" role="list" aria-label="Legacy worker migration preview">
            {workerPreview.records.map((record) => (
              <label key={record.source_id} className={`migration-record ${canSelectWorker(record, replaceExistingConversations) ? "" : "unavailable"}`}>
                <input
                  type="checkbox"
                  checked={selectedWorkers.has(record.source_id)}
                  disabled={!canSelectWorker(record, replaceExistingConversations) || disabled}
                  onChange={() => toggleWorker(record)}
                />
                <span className="migration-record-copy">
                  <strong>{record.name || "Unnamed Legacy worker"}</strong>
                  <small>{record.workspace} · {record.provider === "codex" ? "Codex" : "Claude Code"} · Sleeping</small>
                  <small className={record.conversation_available ? "migration-conversation-found" : "migration-warning"}>{record.conversation_available ? (record.existing_worker_id ? "Exact Legacy conversation found · matching Next worker" : "Exact Legacy conversation found") : "Starts with a fresh conversation"}</small>
                  {record.warnings.map((warning) => <small key={warning} className="migration-warning">{warning}</small>)}
                </span>
              </label>
            ))}
          </div>
          <label className="migration-conversation-choice">
            <input
              type="checkbox"
              checked={resumeLegacyConversations}
              disabled={disabled || !workerPreview.records.some((record) => selectedWorkers.has(record.source_id) && record.conversation_available)}
              onChange={(event) => {
                setResumeLegacyConversations(event.target.checked);
                if (!event.target.checked) {
                  setReplaceExistingConversations(false);
                  setSelectedWorkers((current) => new Set([...current].filter((sourceId) => workerPreview.records.find((record) => record.source_id === sourceId)?.selectable)));
                }
                setConfirmWorkerImport(false);
              }}
            />
            <span><strong>Resume exact Legacy conversations</strong><small>Recommended. On first wake, each eligible worker continues the latest provider conversation for her exact repository. Turn this off to start every imported worker fresh.</small></span>
          </label>
          <label className="migration-conversation-choice">
            <input
              type="checkbox"
              checked={replaceExistingConversations}
              disabled={disabled || !resumeLegacyConversations || !workerPreview.records.some((record) => record.existing_worker_id && record.conversation_available)}
              onChange={(event) => {
                const checked = event.target.checked;
                setReplaceExistingConversations(checked);
                if (!checked) {
                  setSelectedWorkers((current) => new Set([...current].filter((sourceId) => workerPreview.records.find((record) => record.source_id === sourceId)?.selectable)));
                }
                setConfirmWorkerImport(false);
              }}
            />
            <span><strong>Replace conversations on matching workers</strong><small>Optional. Use this when the roster already exists but Legacy has the conversations you want. Selected matching workers must be sleeping; Swarm preserves the current conversation so an untouched migration can be undone.</small></span>
          </label>
          <button type="button" className="primary-action" disabled={disabled || selectedWorkers.size === 0} onClick={() => setConfirmWorkerImport(true)}>Continue to worker import confirmation</button>
        </div>
      )}

      {workerReceipt && !migrationComplete && (
        <div className="migration-receipt" role="status">
          <strong>Familiar crew added safely</strong>
          <p>{workerReceipt.imported_worker_ids.length} worker{workerReceipt.imported_worker_ids.length === 1 ? " is" : "s are"} newly sleeping in the roster, and {workerReceipt.updated_worker_ids?.length ?? 0} matching worker conversation{(workerReceipt.updated_worker_ids?.length ?? 0) === 1 ? " was" : "s were"} replaced. {workerReceipt.resumed_source_ids?.length ?? 0} will resume {(workerReceipt.resumed_source_ids?.length ?? 0) === 1 ? "its" : "their"} exact Legacy conversation on first wake. Review names, repositories, providers, and descriptions before waking anyone.</p>
          {!confirmWorkerRollback ? (
            <button type="button" className="secondary-button" disabled={disabled} onClick={() => setConfirmWorkerRollback(true)}>Undo untouched worker import</button>
          ) : (
            <div className="migration-confirmation">
              <strong>Remove these imported workers?</strong>
              <span>Swarm removes untouched imported workers and restores replaced conversations. It refuses if any affected worker was edited or awakened.</span>
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
            {invalidRecordCount > 0 ? (
              <button type="button" className="text-button" aria-pressed={showInvalidRecords} onClick={() => setShowInvalidRecords((current) => !current)}>
                {showInvalidRecords ? "Hide records needing attention" : `Show ${invalidRecordCount} needing attention`}
              </button>
            ) : null}
            {excludedRecordCount > 0 ? (
              <button type="button" className="text-button" aria-pressed={showExcludedRecords} onClick={() => setShowExcludedRecords((current) => !current)}>
                {showExcludedRecords ? "Hide records staying in Legacy" : `Show ${excludedRecordCount} staying in Legacy`}
              </button>
            ) : null}
            {closedHistoryCount > 0 ? (
              <button type="button" className="text-button" aria-pressed={showClosedHistory} onClick={() => setShowClosedHistory((current) => !current)}>
                {showClosedHistory ? "Hide closed history" : `Show ${closedHistoryCount} closed Legacy task${closedHistoryCount === 1 ? "" : "s"}`}
              </button>
            ) : null}
          </div>
          <div className="migration-records" role="list" aria-label="Legacy task migration preview">
            {visibleTaskRecords.map((record) => (
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
          <button type="button" className="primary-action" disabled={disabled || selected.size === 0} onClick={() => setConfirmImport(true)}>Continue to task import confirmation</button>
        </>
      )}

      {receipt && !migrationComplete && (
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
      {migrationComplete && receipt && workerReceipt && (
        <div className="migration-receipt migration-complete" role="status">
          <strong>Migration complete</strong>
          <p><b>{workerReceipt.imported_worker_ids.length + (workerReceipt.updated_worker_ids?.length ?? 0)}</b> worker{workerReceipt.imported_worker_ids.length + (workerReceipt.updated_worker_ids?.length ?? 0) === 1 ? "" : "s"} and <b>{receipt.imported_task_ids.length}</b> task{receipt.imported_task_ids.length === 1 ? "" : "s"} were brought forward. Workers remain sleeping; imported work remains Draft until you approve it.</p>
          <button type="button" className="primary-action" onClick={onOpenTasks}>Review imported tasks</button>
          <details>
            <summary>Migration details and rollback</summary>
            <p>{workerReceipt.resumed_source_ids?.length ?? 0} worker{(workerReceipt.resumed_source_ids?.length ?? 0) === 1 ? " will" : "s will"} resume an exact Legacy conversation on first wake. Legacy remains unchanged until you explicitly finish its handoff.</p>
            <div className="settings-actions">
              <button type="button" className="secondary-button" onClick={() => downloadJson(receipt, `swarm-next-migration-receipt-${receipt.batch_id}.json`)}>Download migration receipt</button>
              <button type="button" className="secondary-button" disabled={disabled} onClick={() => setConfirmWorkerRollback(true)}>Undo untouched worker import</button>
              <button type="button" className="secondary-button" disabled={disabled} onClick={() => setConfirmRollback(true)}>Undo untouched task import</button>
            </div>
          </details>
        </div>
      )}
      {confirmWorkerImport && (
        <MigrationConfirmationDialog
          title={`Import ${selectedWorkers.size} worker${selectedWorkers.size === 1 ? "" : "s"}?`}
          detail={`Swarm will add the selected roster entries${replaceExistingConversations ? " and replace selected matching conversations" : ""}. Workers remain sleeping; no Claude or Codex process starts. ${resumeLegacyConversations ? "Eligible workers resume their exact Legacy conversation on first wake." : "Workers start fresh."}`}
          confirmLabel={working ? "Importing workers…" : `Import ${selectedWorkers.size} worker${selectedWorkers.size === 1 ? "" : "s"}`}
          disabled={disabled}
          onCancel={() => setConfirmWorkerImport(false)}
          onConfirm={() => void importSelectedWorkers()}
        />
      )}
      {confirmImport && (
        <MigrationConfirmationDialog
          title={`Import ${selected.size} task${selected.size === 1 ? "" : "s"}?`}
          detail="This is the step that changes Swarm. The selected work will appear as Drafts on the task board. No worker starts, and Legacy remains unchanged."
          confirmLabel={working ? "Importing tasks…" : `Import ${selected.size} task${selected.size === 1 ? "" : "s"}`}
          disabled={disabled}
          onCancel={() => setConfirmImport(false)}
          onConfirm={() => void importSelected()}
        />
      )}
      {confirmWorkerRollback && migrationComplete && (
        <MigrationConfirmationDialog
          title="Undo the untouched worker import?"
          detail="Swarm removes untouched imported workers and restores replaced conversations. It refuses if an affected worker was edited or awakened."
          confirmLabel={working ? "Checking…" : "Undo worker import"}
          danger
          disabled={disabled}
          onCancel={() => setConfirmWorkerRollback(false)}
          onConfirm={() => void rollbackWorkers()}
        />
      )}
      {confirmRollback && migrationComplete && (
        <MigrationConfirmationDialog
          title="Undo the untouched task import?"
          detail="Swarm removes only tasks from this import batch that have not changed."
          confirmLabel={working ? "Checking…" : "Undo task import"}
          danger
          disabled={disabled}
          onCancel={() => setConfirmRollback(false)}
          onConfirm={() => void rollback()}
        />
      )}
      {message && <p className="migration-message" role="status">{message}</p>}
      <small className="privacy-note">Migration reads the Legacy crew, open local tasks, and provider conversation identities needed for this preview. Conversation content, credentials, terminal output, identity files, and Jira records are never included.</small>
    </section>
  );
}

function MigrationConfirmationDialog({ title, detail, confirmLabel, disabled, danger = false, onCancel, onConfirm }: {
  title: string;
  detail: string;
  confirmLabel: string;
  disabled: boolean;
  danger?: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  const dialog = useModalFocus<HTMLElement>(onCancel);
  return (
    <div className="task-detail-backdrop" role="presentation" onMouseDown={(event) => { if (event.target === event.currentTarget) onCancel(); }}>
      <section ref={dialog} tabIndex={-1} className="migration-confirm-dialog" role="dialog" aria-modal="true" aria-label="Confirm Legacy migration">
        <p className="eyebrow">Final confirmation</p>
        <h3>{title}</h3>
        <p>{detail}</p>
        <div className="settings-actions">
          <button type="button" className="secondary-button" disabled={disabled} onClick={onCancel}>Go back</button>
          <button type="button" className={danger ? "danger-button" : "primary-action"} disabled={disabled} onClick={onConfirm}>{confirmLabel}</button>
        </div>
      </section>
    </div>
  );
}

function isMissingReceiptEndpoint(error: unknown) {
  return error instanceof RuntimeRequestError && error.status === 404;
}

function canSelectWorker(record: LegacyWorkerPreview, replaceExistingConversations: boolean) {
  return record.selectable
    || Boolean(replaceExistingConversations && record.existing_worker_id && record.conversation_available);
}

function migrationRecordSummary(record: LegacyTaskPreview) {
  if (record.disposition === "skipped_jira") return "Jira issue · returns through Jira sync";
  if (record.disposition === "skipped_closed") return "Closed in Legacy · remains in Legacy history";
  if (record.disposition === "duplicate") return "Already represented in Swarm";
  if (record.disposition === "invalid") return "Cannot import until its source data is repaired";
  const worker = record.matched_worker_name ? ` · ${record.matched_worker_name}` : " · Unassigned";
  return `${record.source_status} → ${record.target_state}${worker}`;
}

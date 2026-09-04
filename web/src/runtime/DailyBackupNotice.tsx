import type { DailyBackupStatus } from "../api";

export default function DailyBackupNotice({ status, onDetails }: { status?: DailyBackupStatus; onDetails: () => void }) {
  if (!status || status.state === "not_reported" || status.state === "ready") return null;
  return <div className="runtime-update-card" role="status">
    <p className="runtime-update-label">{status.state === "failed" ? "Daily backup needs attention" : "Backup status unavailable"}</p>
    <p className="runtime-update-detail">{status.state === "failed"
      ? "The daily snapshot did not complete. Retained backups were left in place; workers were not stopped."
      : "Swarm could not read the daily backup outcome. Backup health is unconfirmed."}</p>
    <button type="button" className="runtime-update-run" onClick={onDetails}>Backup details</button>
  </div>;
}

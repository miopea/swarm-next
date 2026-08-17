import { authenticatedFetch } from "./request";

export type LegacyImportDisposition =
  | "ready"
  | "transformed"
  | "duplicate"
  | "skipped_jira"
  | "skipped_closed"
  | "invalid";

export type LegacyMigrationBundle = {
  format: string;
  version: number;
  source: {
    installation_id: string;
    schema_version?: number;
    exported_at: number;
    snapshot_digest: string;
  };
  tasks: unknown[];
};

export type LegacyTaskPreview = {
  source_id: string;
  title: string;
  source_status: string;
  target_state?: "draft" | "ready" | "active" | "blocked" | "review" | "completed";
  priority: "low" | "normal" | "high" | "urgent";
  matched_worker_id?: string;
  matched_worker_name?: string;
  disposition: LegacyImportDisposition;
  selectable: boolean;
  warnings: string[];
};

export type LegacyMigrationPreview = {
  bundle_digest: string;
  source_installation_id: string;
  records: LegacyTaskPreview[];
  selectable: number;
  skipped: number;
  invalid: number;
};

export type LegacyMigrationReceipt = {
  batch_id: string;
  bundle_digest: string;
  source_installation_id: string;
  source_snapshot_digest: string;
  imported_task_ids: string[];
  imported_source_ids: string[];
  imported_at: number;
};

export async function listActiveLegacyTaskMigrations(
  operatorToken: string,
): Promise<LegacyMigrationReceipt[]> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/migrations/legacy/tasks");
  return response.json() as Promise<LegacyMigrationReceipt[]>;
}

export async function previewLegacyTaskMigration(
  operatorToken: string,
  bundle: LegacyMigrationBundle,
): Promise<LegacyMigrationPreview> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/migrations/legacy/tasks/preview", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(bundle),
  });
  return response.json() as Promise<LegacyMigrationPreview>;
}

export async function commitLegacyTaskMigration(
  operatorToken: string,
  bundle: LegacyMigrationBundle,
  preview: LegacyMigrationPreview,
  selectedSourceIds: string[],
): Promise<LegacyMigrationReceipt> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/migrations/legacy/tasks/commit", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      bundle,
      commit: {
        bundle_digest: preview.bundle_digest,
        selected_source_ids: selectedSourceIds,
      },
    }),
  });
  return response.json() as Promise<LegacyMigrationReceipt>;
}

export async function rollbackLegacyTaskMigration(
  operatorToken: string,
  receipt: LegacyMigrationReceipt,
): Promise<{ batch_id: string; removed_tasks: number; rolled_back_at: number }> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/migrations/legacy/tasks/rollback", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      batch_id: receipt.batch_id,
      bundle_digest: receipt.bundle_digest,
    }),
  });
  return response.json() as Promise<{ batch_id: string; removed_tasks: number; rolled_back_at: number }>;
}

CREATE TABLE queen_task_review_receipts_legacy (
    task_id TEXT PRIMARY KEY NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    run_id TEXT NOT NULL,
    kind TEXT NOT NULL CHECK(kind IN ('external_condition','operator_deferral')),
    accepted_revision TEXT NOT NULL CHECK(length(accepted_revision)=64),
    input_payload TEXT NOT NULL CHECK(length(input_payload)<=32768),
    recorded_sequence INTEGER NOT NULL REFERENCES task_activity(sequence),
    recorded_at INTEGER NOT NULL
);
INSERT INTO queen_task_review_receipts_legacy SELECT * FROM queen_task_review_receipts;
DROP TABLE queen_task_review_receipts;
ALTER TABLE queen_task_review_receipts_legacy RENAME TO queen_task_review_receipts;

-- Test-only: the Jira write queue and Needs-you deliveries as they were before
-- schema 207 (ADR 0120). Neither table can lose a CHECK value in place, so both
-- are rebuilt, keeping every row the older shape accepts.
ALTER TABLE jira_project_bindings DROP COLUMN confirm_resolution;
CREATE TABLE jira_transition_deliveries_undo (
    id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    target_task_state TEXT NOT NULL CHECK (
        target_task_state IN ('draft','ready','active','blocked','review','completed')
    ),
    state TEXT NOT NULL CHECK (
        state IN ('queued','dispatching','delivered','conflict','uncertain')
    ),
    attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0 AND attempts <= 3),
    available_at INTEGER NOT NULL DEFAULT (unixepoch()),
    attempted_at INTEGER,
    delivered_at INTEGER,
    last_error TEXT,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
);
INSERT INTO jira_transition_deliveries_undo
SELECT id, task_id, target_task_state, state, attempts, available_at, attempted_at,
       delivered_at, last_error, created_at, updated_at
FROM jira_transition_deliveries
WHERE state IN ('queued','dispatching','delivered','conflict','uncertain');
DROP TABLE jira_transition_deliveries;
ALTER TABLE jira_transition_deliveries_undo RENAME TO jira_transition_deliveries;
CREATE UNIQUE INDEX jira_transition_one_active_per_task
    ON jira_transition_deliveries(task_id)
    WHERE state IN ('queued','dispatching');
CREATE INDEX jira_transition_delivery_queue
    ON jira_transition_deliveries(state, available_at, updated_at);
CREATE TABLE notification_deliveries_undo (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    operator_id TEXT NOT NULL REFERENCES operators(id) ON DELETE CASCADE,
    subscription_id TEXT NOT NULL
        REFERENCES notification_subscriptions(device_id) ON DELETE CASCADE,
    decision_id TEXT REFERENCES decision_requests(id) ON DELETE CASCADE,
    subject_key TEXT NOT NULL,
    urgency TEXT NOT NULL CHECK (urgency IN ('normal','time_sensitive')),
    kind TEXT NOT NULL CHECK (kind IN (
        'decision','assist','queen_automation','held_delivery',
        'email_reply','test'
    )),
    state TEXT NOT NULL CHECK (state IN ('queued','dispatching')),
    attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts BETWEEN 0 AND 5),
    available_at INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    CHECK ((kind = 'decision' AND decision_id IS NOT NULL)
           OR (kind <> 'decision' AND decision_id IS NULL)),
    UNIQUE(subject_key, subscription_id)
);
INSERT INTO notification_deliveries_undo
SELECT id, operator_id, subscription_id, decision_id, subject_key, urgency, kind, state,
       attempts, available_at, created_at
FROM notification_deliveries WHERE kind <> 'jira_resolution';
DROP TABLE notification_deliveries;
ALTER TABLE notification_deliveries_undo RENAME TO notification_deliveries;

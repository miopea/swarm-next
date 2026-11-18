-- Test-only: Needs-you deliveries as they were before schema 229 (ADR 0175).
-- A CHECK cannot lose a value in place, so the table is rebuilt, keeping every
-- row the older shape accepts.
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
        'email_reply','jira_resolution','test'
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
FROM notification_deliveries WHERE kind <> 'github_review';
DROP TABLE notification_deliveries;
ALTER TABLE notification_deliveries_undo RENAME TO notification_deliveries;

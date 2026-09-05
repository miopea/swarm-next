-- Test-only reconstruction of the schema-136 table, preserving its existing rows.
-- The real migration's populated-row test separately verifies child preservation.
PRAGMA foreign_keys = OFF;
CREATE TABLE decision_requests_v136 (
 id TEXT PRIMARY KEY,
 hive_id TEXT NOT NULL REFERENCES hives(id),
 requesting_worker_id TEXT NOT NULL REFERENCES worker_profiles(id),
 task_id TEXT REFERENCES tasks(id),
 kind TEXT NOT NULL CHECK (kind IN ('input','approval','credentials','conflict','help')),
 urgency TEXT NOT NULL CHECK (urgency IN ('normal','time_sensitive')),
 title TEXT NOT NULL, reason TEXT NOT NULL, risk TEXT NOT NULL, evidence TEXT NOT NULL,
 suggested_action TEXT NOT NULL, allowed_actions TEXT NOT NULL, deadline INTEGER,
 state TEXT NOT NULL DEFAULT 'pending' CHECK (state IN ('pending','resolved')),
 resolution_action TEXT, resolution_note TEXT NOT NULL DEFAULT '',
 resolved_by_operator_id TEXT REFERENCES operators(id),
 created_at INTEGER NOT NULL DEFAULT (unixepoch()),
 updated_at INTEGER NOT NULL DEFAULT (unixepoch()), resolved_at INTEGER,
 resolution_surface TEXT NOT NULL DEFAULT '', questions TEXT NOT NULL DEFAULT '[]',
 resolution_answers TEXT NOT NULL DEFAULT '{}', summary TEXT NOT NULL DEFAULT '', requested_command TEXT,
 CHECK ((state = 'pending' AND resolution_action IS NULL AND resolved_at IS NULL)
     OR (state = 'resolved' AND resolution_action IS NOT NULL AND resolved_at IS NOT NULL))
);
INSERT INTO decision_requests_v136
 SELECT id,hive_id,requesting_worker_id,task_id,kind,urgency,title,reason,risk,evidence,
 suggested_action,allowed_actions,deadline,state,resolution_action,resolution_note,
 resolved_by_operator_id,created_at,updated_at,resolved_at,resolution_surface,questions,
 resolution_answers,summary,requested_command FROM decision_requests;
DROP TABLE decision_requests;
ALTER TABLE decision_requests_v136 RENAME TO decision_requests;
CREATE INDEX decision_requests_inbox ON decision_requests(hive_id,state,urgency,deadline,created_at DESC);
CREATE INDEX decision_requests_by_worker ON decision_requests(requesting_worker_id,state,created_at DESC);
PRAGMA foreign_keys = ON;

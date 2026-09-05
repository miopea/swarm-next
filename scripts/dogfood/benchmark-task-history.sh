#!/bin/sh
# Compare the existing correlated lookup with a task-scoped history index.
# Only a disposable copy is indexed. Never run CREATE INDEX against the Hive.
set -eu
umask 077
source_db=${1:?verified backup path is required}
[ -f "$source_db" ] || exit 2
drill=$(mktemp -d /tmp/swarm-task-history.XXXXXXXX)
cleanup() {
  case "$drill" in /tmp/swarm-task-history.*)
    rm -f -- "$drill/candidate.sqlite3"
    rmdir -- "$drill"
  ;; esac
}
trap cleanup EXIT HUP INT TERM
cp -- "$source_db" "$drill/candidate.sqlite3"
sqlite3 "$drill/candidate.sqlite3" <<'SQL'
.timer on
SELECT 'before', count(*) FROM task_activity;
EXPLAIN QUERY PLAN SELECT t.id, EXISTS(SELECT 1 FROM task_activity a WHERE a.task_id=t.id AND a.actor_kind='worker') FROM tasks t WHERE t.removed_at IS NULL;
SELECT sum(EXISTS(SELECT 1 FROM task_activity a WHERE a.task_id=t.id AND a.actor_kind='worker')) FROM tasks t WHERE t.removed_at IS NULL;
SELECT sum(EXISTS(SELECT 1 FROM task_activity a WHERE a.task_id=t.id AND a.actor_kind='worker')) FROM tasks t WHERE t.removed_at IS NULL;
SELECT sum(EXISTS(SELECT 1 FROM task_activity a WHERE a.task_id=t.id AND a.actor_kind='worker')) FROM tasks t WHERE t.removed_at IS NULL;
CREATE INDEX task_activity_by_task_sequence ON task_activity(task_id,sequence);
SELECT 'after', count(*) FROM task_activity;
EXPLAIN QUERY PLAN SELECT t.id, EXISTS(SELECT 1 FROM task_activity a WHERE a.task_id=t.id AND a.actor_kind='worker') FROM tasks t WHERE t.removed_at IS NULL;
SELECT sum(EXISTS(SELECT 1 FROM task_activity a WHERE a.task_id=t.id AND a.actor_kind='worker')) FROM tasks t WHERE t.removed_at IS NULL;
SELECT sum(EXISTS(SELECT 1 FROM task_activity a WHERE a.task_id=t.id AND a.actor_kind='worker')) FROM tasks t WHERE t.removed_at IS NULL;
SELECT sum(EXISTS(SELECT 1 FROM task_activity a WHERE a.task_id=t.id AND a.actor_kind='worker')) FROM tasks t WHERE t.removed_at IS NULL;
SELECT 'other task lookups before';
SELECT sum(EXISTS(SELECT 1 FROM decision_requests d WHERE d.task_id=t.id AND d.state='pending')),
       sum((SELECT state FROM task_outcome_deliveries o WHERE o.task_id=t.id AND o.target_state=t.state ORDER BY o.activity_sequence DESC LIMIT 1) IS NOT NULL)
FROM tasks t WHERE t.removed_at IS NULL;
CREATE INDEX decision_requests_pending_by_task ON decision_requests(task_id) WHERE state='pending';
CREATE INDEX task_outcomes_by_task_state ON task_outcome_deliveries(task_id,target_state,activity_sequence DESC);
SELECT 'other task lookups after';
SELECT sum(EXISTS(SELECT 1 FROM decision_requests d WHERE d.task_id=t.id AND d.state='pending')),
       sum((SELECT state FROM task_outcome_deliveries o WHERE o.task_id=t.id AND o.target_state=t.state ORDER BY o.activity_sequence DESC LIMIT 1) IS NOT NULL)
FROM tasks t WHERE t.removed_at IS NULL;
PRAGMA quick_check;
SQL
exit 0

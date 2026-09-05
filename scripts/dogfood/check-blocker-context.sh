#!/bin/sh
# Exercise only an unassigned, disposable task through the authenticated API.
set -eu
base=http://127.0.0.1:8766
required_revision=${1:?expected runtime revision is required}
curl --fail --silent --show-error "$base/health" |
  jq -e --arg revision "$required_revision" '.status == "ok" and (.version | contains($revision))' >/dev/null
token=$(sed -n 's/^SWARM_OPERATOR_TOKEN=//p' "$HOME/.config/swarm/swarm.env")
api() {
  printf 'header = "Authorization: Bearer %s"\n' "$token" |
    curl --config - --fail --silent --show-error --max-time 30 "$@"
}
task=$(api -X POST -H 'Content-Type: application/json' --data '{"title":"Dogfood: current blocker projection lifecycle","workspace":"/home/bschleifer/projects/.swarm-next-dogfood/workflow-fixture","description":"Disposable API lifecycle check only. Do not assign, execute, or deploy. The test runner will abandon this task when verification finishes."}' "$base/api/v1/tasks" | jq -er '.id')
printf 'Isolated blocker task=%s\n' "$task"
transition() {
  api -X PATCH -H 'Content-Type: application/json' --data "$1" "$base/api/v1/tasks/$task/state"
}
transition '{"state":"ready"}' | jq -e '.assigned_worker_id == null' >/dev/null
transition '{"state":"active"}' | jq -e '.blocked_note == null' >/dev/null
transition '{"state":"blocked","note":"Waiting for the isolated contract fixture"}' |
  jq -e '.blocked_note == "Waiting for the isolated contract fixture"' >/dev/null
api "$base/api/v1/tasks" | jq -e --arg id "$task" '.[] | select(.id == $id) | .blocked_note == "Waiting for the isolated contract fixture"' >/dev/null
transition '{"state":"active"}' | jq -e '.blocked_note == null' >/dev/null
transition '{"state":"blocked"}' | jq -e '.blocked_note == null' >/dev/null
transition '{"state":"abandoned","note":"Isolated blocker projection lifecycle passed; no work or deployment requested."}' |
  jq -e '.state == "abandoned" and .blocked_note == null' >/dev/null
api "$base/api/v1/tasks/settled" | jq -e --arg id "$task" '.[] | select(.id == $id) | .state == "abandoned" and .blocked_note == null' >/dev/null
printf 'Blocker context appeared, cleared, stayed absent on reblock, and test task was abandoned.\n'

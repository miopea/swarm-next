#!/bin/sh
# Isolated operator/API dependency drill. Never assigns or starts a worker.
set -eu
base=http://127.0.0.1:8766
revision=${1:?expected runtime revision is required}
curl --fail --silent --show-error "$base/health" |
  jq -e --arg revision "$revision" '.status == "ok" and (.version | contains($revision))' >/dev/null
token=$(sed -n 's/^SWARM_OPERATOR_TOKEN=//p' "$HOME/.config/swarm/swarm.env")
api() {
  printf 'header = "Authorization: Bearer %s"\n' "$token" |
    curl --config - --fail --silent --show-error --max-time 30 "$@"
}
workspace=/home/bschleifer/projects/.swarm-next-dogfood/workflow-fixture
title="Dogfood: explicit prerequisite drill $revision"
existing=$( { api "$base/api/v1/tasks"; api "$base/api/v1/tasks/settled"; } |
  jq -sr --arg title "$title" '[.[][] | select(.title | startswith($title))] | unique_by(.id) | length')
[ "$existing" = 0 ] || { echo 'Existing drill tasks found; inspect them instead of duplicating.'; exit 2; }
create() {
  body=$(jq -nc --arg title "$title $1" --arg workspace "$workspace" '{title:$title,workspace:$workspace,description:"Disposable operator/API prerequisite verification only. Do not assign, execute or deploy. The runner will abandon this task after checking explicit links and guarded transitions."}')
  api -X POST -H 'Content-Type: application/json' --data "$body" "$base/api/v1/tasks" | jq -er '.id'
}
consumer=$(create consumer)
upstream=$(create upstream)
printf 'consumer=%s upstream=%s\n' "$consumer" "$upstream"
transition() {
  api -X PATCH -H 'Content-Type: application/json' --data "$(jq -nc --arg state "$2" --arg note "$3" '{state:$state,note:$note}')" "$base/api/v1/tasks/$1/state"
}
edge() {
  api -X POST -H 'Content-Type: application/json' --data "$(jq -nc --arg id "$upstream" --arg operation "$1" '{prerequisite_id:$id,operation:$operation,reason:"Isolated API prerequisite drill"}')" "$base/api/v1/tasks/$consumer/prerequisites"
}
for task in "$consumer" "$upstream"; do
  transition "$task" ready 'Isolated drill setup' | jq -e '.assigned_worker_id == null' >/dev/null
  transition "$task" blocked 'Waiting for the isolated API check' >/dev/null
done
edge add | jq -e --arg id "$upstream" '.state == "blocked" and (.prerequisites | length) == 1 and .prerequisites[0].prerequisite_id == $id' >/dev/null
edge add | jq -e '(.prerequisites | length) == 1' >/dev/null
status=$(api --output /dev/null --write-out '%{http_code}' -X PATCH -H 'Content-Type: application/json' --data '{"state":"ready"}' "$base/api/v1/tasks/$consumer/state" 2>/dev/null) || [ "$status" = 409 ]
[ "$status" = 409 ]
status=$(api --output /dev/null --write-out '%{http_code}' -X POST -H 'Content-Type: application/json' --data "$(jq -nc --arg id "$consumer" '{prerequisite_id:$id,operation:"add",reason:"Cycle must be refused"}')" "$base/api/v1/tasks/$upstream/prerequisites" 2>/dev/null) || [ "$status" = 409 ]
[ "$status" = 409 ]
transition "$upstream" abandoned 'Drill upstream abandoned; this must not satisfy its prerequisite' >/dev/null
api "$base/api/v1/tasks" | jq -e --arg id "$consumer" '.[] | select(.id == $id) | .state == "blocked" and .prerequisites[0].state == "abandoned"' >/dev/null
edge remove | jq -e '(.prerequisites // [] | length) == 0 and .state == "blocked"' >/dev/null
transition "$consumer" ready 'Explicit edge removal permits ordinary resumption' >/dev/null
transition "$consumer" abandoned 'Isolated prerequisite drill passed; no work or deployment was requested' >/dev/null
api "$base/api/v1/tasks/settled" | jq -e --arg a "$consumer" --arg b "$upstream" '[.[] | select(.id == $a or .id == $b) | select(.state == "abandoned")] | length == 2' >/dev/null
echo 'Prerequisite persistence, idempotency, refusal, abandonment, removal and ordinary resumption passed. Both drill tasks are abandoned with audit history.'

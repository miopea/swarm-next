#!/usr/bin/env bash
set -euo pipefail
# Two fictional read-only tasks. No real backlog edits, releases or direct PTY input.
# A partial setup is reported and must be inspected, never silently duplicated.
base=http://127.0.0.1:8766
prefix='Dogfood recovery 8e3d6647'
token=$(sed -n 's/^SWARM_OPERATOR_TOKEN=//p' "$HOME/.config/swarm/swarm.env")
[[ ${#token} -ge 32 ]] || exit 2
api() {
  printf 'header = "Authorization: Bearer %s"\n' "$token" |
    curl --config - --fail --silent --show-error --max-time 30 "$@"
}
scenario() {
  { api "$base/api/v1/tasks"; api "$base/api/v1/tasks/settled"; } |
    jq -s --arg prefix "$prefix" '[.[][] | select(.title | startswith($prefix))] | unique_by(.id)'
}
if [[ ${1:-} == --status ]]; then
  scenario | jq '[.[] | {id,title,state,next_move_owner,assigned_worker_id,dispatch_state,closed_on_evidence}]'
  exit 0
fi
[[ ${1:-} == --setup ]] || { echo 'Use --setup or --status'; exit 2; }
existing=$(scenario)
[[ $(jq length <<<"$existing") == 0 ]] || { echo 'Scenario already exists; inspect --status instead of duplicating'; exit 1; }
profiles=$(api "$base/api/v1/workers")
jq -e '[.[] | select((.id=="01a06eda-bdd1-7a82-928e-cffbee0be6c1" and .workspace=="/home/bschleifer/projects/.swarm-next-dogfood/workflow-fixture") or (.id=="01a07193-7d79-7113-93f4-8e9a43db6964" and .workspace=="/home/bschleifer/projects/.swarm-next-dogfood/contract-fixture"))] | length==2' <<<"$profiles" >/dev/null
up_body=$(jq -nc --arg title "$prefix: upstream read-only verification" '{title:$title,workspace:"/home/bschleifer/projects/.swarm-next-dogfood/workflow-fixture",description:"Isolated Swarm maturity acceptance. Read this demo repository and run its existing local Node tests (node --test where applicable). Do not edit files, commit, deploy, contact external systems, or leave this repository. Report actual results, an empty task commit list and truthful no-deployment investigation evidence, then Review. Routine machine-supported evidence should settle automatically; do not ask Queen approval solely to finish. If tests fail, report the exact failure without claiming success."}')
up=$(api -X POST -H 'Content-Type: application/json' --data "$up_body" "$base/api/v1/tasks" | jq -er '.id')
echo "UPSTREAM=$up"
down_body=$(jq -nc --arg title "$prefix: downstream dependency pickup" --arg up "$up" '{title:$title,workspace:"/home/bschleifer/projects/.swarm-next-dogfood/contract-fixture",description:("Isolated acceptance. Explicit prerequisite " + $up + " must complete first. Queen owns checking its recorded completion and routing this task; do not bypass the link. Then read this demo repository and run its existing local Node tests (node --test where applicable). Do not edit, commit, deploy, use external systems or leave this repository. Report actual results, an empty task commit list and truthful no-deployment investigation evidence, then Review. Routine verified completion does not require Queen approval. Report exact failures honestly.")}')
down=$(api -X POST -H 'Content-Type: application/json' --data "$down_body" "$base/api/v1/tasks" | jq -er '.id')
echo "DOWNSTREAM=$down"
api -X PATCH -H 'Content-Type: application/json' --data '{"state":"ready","note":"Prepare isolated dependency fixture"}' "$base/api/v1/tasks/$down/state" >/dev/null
api -X PATCH -H 'Content-Type: application/json' --data '{"state":"blocked","note":"Explicit isolated upstream verification prerequisite"}' "$base/api/v1/tasks/$down/state" >/dev/null
api -X POST -H 'Content-Type: application/json' --data "$(jq -nc --arg id "$up" '{prerequisite_id:$id,operation:"add",reason:"Exact fictional upstream verification required by this acceptance scenario"}')" "$base/api/v1/tasks/$down/prerequisites" >/dev/null
api -X PUT -H 'Content-Type: application/json' --data '{"worker_id":"01a07193-7d79-7113-93f4-8e9a43db6964"}' "$base/api/v1/tasks/$down/assignment" >/dev/null
api -X PATCH -H 'Content-Type: application/json' --data '{"state":"ready","note":"Run isolated verification through normal guarded dispatch"}' "$base/api/v1/tasks/$up/state" >/dev/null
api -X PUT -H 'Content-Type: application/json' --data '{"worker_id":"01a06eda-bdd1-7a82-928e-cffbee0be6c1"}' "$base/api/v1/tasks/$up/assignment" >/dev/null
scenario | jq '[.[] | {id,title,state,next_move_owner,assigned_worker_id,dispatch_state}]'

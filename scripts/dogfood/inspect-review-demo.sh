#!/usr/bin/env bash
set -euo pipefail
# Inspect one disposable review exercise; --wake starts only its worker and
# --retry assigns one bounded verification task through normal guarded delivery.
# Credentials are never emitted. No task or decision is resolved by this script.
base=http://127.0.0.1:8766
worker=01a06eda-bdd1-7a82-928e-cffbee0be6c1
task=01a06ee5-adc7-7fe2-9fa8-d6755aedbc25
token=$(sed -n 's/^SWARM_OPERATOR_TOKEN=//p' "$HOME/.config/swarm/swarm.env")
[[ ${#token} -ge 32 ]] || exit 2
api() {
  printf 'header = "Authorization: Bearer %s"\n' "$token" |
    curl --config - --fail --silent --show-error --max-time 30 "$@"
}
if [[ ${1:-} == --wake ]]; then
  profile=$(api "$base/api/v1/workers" | jq -ce --arg id "$worker" '.[] | select(.id == $id)')
  jq -e '.name == "Swarm Dogfood" and .workspace == "/home/bschleifer/projects/.swarm-next-dogfood/workflow-fixture" and .provider == "claude_code"' <<<"$profile" >/dev/null
  if ! jq -e '.running' <<<"$profile" >/dev/null; then
    api -X POST -H 'Content-Type: application/json' --data '{"rows":40,"columns":120}' "$base/api/v1/workers/$worker/start"
  fi
fi
if [[ ${1:-} == --retry ]]; then
  title='Dogfood: retry local-code completion after 40c4f74a'
  workspace=/home/bschleifer/projects/.swarm-next-dogfood/workflow-fixture
  profile=$(api "$base/api/v1/workers" | jq -ce --arg id "$worker" '.[] | select(.id == $id)')
  jq -e --arg workspace "$workspace" '.name == "Swarm Dogfood" and .workspace == $workspace and .provider == "claude_code"' <<<"$profile" >/dev/null
  existing=$( { api "$base/api/v1/tasks"; api "$base/api/v1/tasks/settled"; } | jq -sr --arg title "$title" '[.[][] | select(.title == $title)] | length')
  [[ $existing == 0 ]] || { echo 'Retry task already exists; inspect it instead of duplicating.'; exit 0; }
  body=$(jq -nc --arg title "$title" --arg workspace "$workspace" --arg task "$task" '{title:$title,workspace:$workspace,priority:"normal",description:("Isolated dogfood verification only. Runtime 40c4f74a now allows code no-deployment claims for Queen judgment. Recheck the existing heartbeat task " + $task + ": inspect its commit and run node --test; make no code changes, deployments or commits. Retry swarm_record_no_deployment on that original task with the truthful local-only scope. If accepted, message Queen on that original task asking her to recheck the existing evidence, approve the claim if supported, and reconcile her now-obsolete operator escalation. Do not claim operator approval or resolve that decision yourself. Record success or exact failure. For this verification task report an empty commit list and a truthful investigation outcome, then Review.")}')
  retry=$(api -H 'Content-Type: application/json' -X POST --data "$body" "$base/api/v1/tasks" | jq -er '.id')
  echo "created verification task=$retry"
  api -H 'Content-Type: application/json' -X PATCH --data '{"state":"ready"}' "$base/api/v1/tasks/$retry/state" >/dev/null
  api -H 'Content-Type: application/json' -X PUT --data "$(jq -nc --arg id "$worker" '{worker_id:$id}')" "$base/api/v1/tasks/$retry/assignment" >/dev/null
fi
if [[ ${1:-} == --status ]]; then
  api "$base/api/v1/workers" | jq --arg id "$worker" '.[] | select(.id == $id) | {id,running,attention_state,active_session_id}'
  { api "$base/api/v1/tasks"; api "$base/api/v1/tasks/settled"; } |
    jq -s --arg worker "$worker" '[.[][] | select(.assigned_worker_id == $worker) | {id,title,state,next_move_owner,dispatch_state,closed_on_evidence,closed_unverifiable}] | unique_by(.id)'
  api "$base/api/v1/decisions" | jq --arg id "$task" '.[] | select(.task_id == $id) | {id,state,discharge}'
  exit 0
fi
api "$base/api/v1/workers" | jq --arg id "$worker" '.[] | select(.id == $id)'
api "$base/api/v1/tasks" | jq --arg id "$task" '.[] | select(.id == $id)'
api "$base/api/v1/tasks/settled" | jq --arg id "$task" '.[] | select(.id == $id)'
api "$base/api/v1/decisions" | jq --arg id "$task" '.[] | select(.task_id == $id)'
if [[ ${1:-} == --terminal ]]; then
  session=$(api "$base/api/v1/workers" | jq -er --arg id "$worker" '.[] | select(.id == $id) | .active_session_id')
  api "$base/api/v1/terminal/sessions/$session/output" |
    jq -r '.. | objects | select(has("bytes")) | .bytes | implode' | tail -c 7000
fi
exit 0

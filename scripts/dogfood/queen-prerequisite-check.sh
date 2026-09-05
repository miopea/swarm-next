#!/usr/bin/env bash
set -euo pipefail
# Start one isolated worker-to-Queen dependency journey, never an operator edge.
# Re-running inspects the existing task instead of creating another exercise.
base=http://127.0.0.1:8766
consumer=01a06eda-bdd1-7a82-928e-cffbee0be6c1
workspace=/home/bschleifer/projects/.swarm-next-dogfood/workflow-fixture
upstream=/home/bschleifer/projects/.swarm-next-dogfood/contract-fixture
title='Dogfood: Queen-owned cross-worker prerequisite journey'
token=$(sed -n 's/^SWARM_OPERATOR_TOKEN=//p' "$HOME/.config/swarm/swarm.env")
[[ ${#token} -ge 32 ]] || exit 2
api() {
  printf 'header = "Authorization: Bearer %s"\n' "$token" |
    curl --config - --fail --silent --show-error --max-time 30 -H 'Content-Type: application/json' "$@"
}
existing=$({ api "$base/api/v1/tasks"; api "$base/api/v1/tasks/settled"; } |
  jq -sc --arg title "$title" '[.[][] | select(.title == $title)] | unique_by(.id)')
if [[ $(jq length <<<"$existing") != 0 ]]; then
  jq '[.[] | {id,state,assigned_worker_id,prerequisites,next_move_owner}]' <<<"$existing"
  exit 0
fi
profile=$(api "$base/api/v1/workers" | jq -ce --arg id "$consumer" '.[] | select(.id == $id)')
jq -e --arg workspace "$workspace" '.name == "Swarm Dogfood" and .workspace == $workspace and .provider == "claude_code" and .attention_state == "resting" and .engaged_device_id == null' <<<"$profile" >/dev/null
api "$base/api/v1/tasks" | jq -e --arg id "$consumer" '[.[] | select(.assigned_worker_id == $id)] | length == 0' >/dev/null
mkdir -p "$upstream"
git -C "$upstream" init -q
target=$(api "$base/api/v1/workers" | jq -c --arg path "$upstream" '[.[] | select(.workspace == $path)]')
if [[ $(jq length <<<"$target") == 0 ]]; then
  api -X POST --data "$(jq -nc --arg workspace "$upstream" '{name:"Swarm Dogfood Contract",workspace:$workspace,provider:"claude_code",autostart:false}')" "$base/api/v1/workers" >/dev/null
elif [[ $(jq length <<<"$target") != 1 ]] || ! jq -e '.[0].name == "Swarm Dogfood Contract" and .[0].provider == "claude_code"' <<<"$target" >/dev/null; then
  echo 'Unexpected upstream worker; no task created.' >&2
  exit 2
fi
body=$(jq -nc --arg title "$title" --arg workspace "$workspace" --arg upstream "$upstream" '{title:$title,workspace:$workspace,priority:"normal",operator_instruction:"Isolated dogfood exercise only. No releases, deployments, or changes to real projects.",description:("Write dependency-consumer-check.md in this repository documenting the version and field names in " + $upstream + "/contract.md. The contract does not exist yet and belongs to Swarm Dogfood Contract, not you. Do not write outside your repository or create/assign peer work. Report this task Blocked with the missing upstream contract as the reason, and ask Queen to create and assign a documentation-only task to that worker to define version 1 with fields status and checked_at. Ask Queen to record an explicit task prerequisite using swarm_set_task_prerequisite, then resume this task only after the upstream task completes. Queen owns wake/assignment/coordination. When resumed, read the contract, write and commit only your documentation, record truthful documentation-only evidence and report Review. No human decision is needed within this scope. Do not fabricate completion or bypass the dependency.")}')
task=$(api -X POST --data "$body" "$base/api/v1/tasks" | jq -er '.id')
echo "consumer_task=$task"
api -X PATCH --data '{"state":"ready"}' "$base/api/v1/tasks/$task/state" >/dev/null
api -X PUT --data "$(jq -nc --arg id "$consumer" '{worker_id:$id}')" "$base/api/v1/tasks/$task/assignment" >/dev/null
echo 'Assigned through normal guarded delivery; inspect Queen-created tasks and edges before claiming acceptance.'

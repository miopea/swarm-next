#!/usr/bin/env bash
set -euo pipefail
# Isolated missing-input recovery; no production task edits or direct PTY input.
mode=${1:---status}
[[ $mode == --start || $mode == --status ]] || exit 2
base=http://127.0.0.1:8766
worker=01a06eda-bdd1-7a82-928e-cffbee0be6c1
workspace=/home/bschleifer/projects/.swarm-next-dogfood/workflow-fixture
title='DOGFOOD: Queen recovery needs a fictional specimen choice 20260911'
token=$(sed -n 's/^SWARM_OPERATOR_TOKEN=//p' "$HOME/.config/swarm/swarm.env")
[[ ${#token} -ge 32 ]] || exit 2
api() { printf 'header = "Authorization: Bearer %s"\n' "$token" | curl --config - --fail --silent --show-error --max-time 30 --header 'Content-Type: application/json' "$@"; }
profile=$(api "$base/api/v1/workers" | jq -ce --arg id "$worker" '.[] | select(.id == $id)')
jq -e --arg workspace "$workspace" '.name == "Swarm Dogfood" and .workspace == $workspace and .provider == "claude_code"' <<<"$profile" >/dev/null
board=$(api "$base/api/v1/tasks")
matches=$({ printf '%s\n' "$board"; api "$base/api/v1/tasks/settled"; } | jq -sc --arg title "$title" --arg workspace "$workspace" '[.[][] | select(.title == $title and .workspace == $workspace)] | unique_by(.id)')
[[ $(jq length <<<"$matches") -le 1 ]] || { echo 'Duplicate fixture; untouched'; exit 2; }
task=$(jq -r '.[0].id // empty' <<<"$matches")
if [[ $mode == --start && -z $task ]]; then
  jq -e --arg worker "$worker" '[.[] | select(.assigned_worker_id == $worker)] | length == 0' <<<"$board" >/dev/null
  [[ -z $(git -C "$workspace" status --porcelain) ]] || { echo 'Demo has changes; untouched'; exit 3; }
  [[ ! -e "$workspace/operator-specimen-20260911.json" ]] || { echo 'Missing-input fixture already supplied; untouched'; exit 3; }
  description='FICTIONAL DOGFOOD ONLY. Validate the operator-selected specimen for this isolated task: specimen A means amber, specimen B means blue. The selection should be in operator-specimen-20260911.json at this demo repository root. No default selection is authorized by this task. Preserve all existing files and conversations. No writes, commits, deployments, external services, credentials, other workers or real project work. The controller may later answer one fictional operator decision; do not treat that future intention as a present answer.

Worker: first check the exact input path and read this repository README for an explicitly applicable existing default. Do not search outside the repository, guess the choice, manufacture a file, or loop waiting for one. If no selection is established, send Queen one task-linked finding with the exact checks/results and why the selection cannot be recovered within this scope. Do not create your own operator decision: this exercises Queen-owned recovery and escalation. Remain on this same task/conversation awaiting the ordinary response. Answer one concrete follow-up from Queen if needed, but do not repeat generic nudges.

Queen: use the ordinary worker-first safe recovery policy. The task authorizes checking the missing input, not choosing it. If the worker evidence establishes that recovery cannot supply a valid choice, ask the operator once through a concise task-linked DOGFOOD Needs You request: identify the missing selection, the worker checks, and your recommendation. Offer specimen A (amber), specimen B (blue), and cancellation. Do not involve Scout/other workers, convert an absence into authorization, fabricate an external source, or repeatedly ask the worker to check the same absent input. Preserve assignment and link the request structurally so this task is not a silent prose-only blocker.

After a real saved resolution arrives through Swarm, the same worker reads it and performs the corresponding read-only Node assertion using the chosen specimen and expected colour. Record exact check output, empty commit list and truthful no-deployment investigation evidence; submit Review through normal settlement. Cancellation instead means cancel this fictional task, with no work elsewhere. A supplied decision is the input; no disk file needs to be created.'
  task=$(api -X POST --data "$(jq -nc --arg title "$title" --arg workspace "$workspace" --arg description "$description" '{title:$title,workspace:$workspace,description:$description,priority:"normal"}')" "$base/api/v1/tasks" | jq -er '.id')
  echo "Created fixture=$task"
fi
[[ -n $task ]] || { jq '{id,running,attention_state,active_session_id}' <<<"$profile"; echo 'No fixture exists'; exit 0; }
read_task() { { api "$base/api/v1/tasks"; api "$base/api/v1/tasks/settled"; } | jq -sce --arg id "$task" '[.[][] | select(.id == $id)] | unique_by(.id) | if length == 1 then .[0] else error("fixture missing") end'; }
if [[ $mode == --start ]]; then
  record=$(read_task)
  jq -e --arg worker "$worker" '.assigned_worker_id == null or .assigned_worker_id == $worker' <<<"$record" >/dev/null
  if [[ $(jq -r .state <<<"$record") == draft ]]; then api -X PATCH --data '{"state":"ready"}' "$base/api/v1/tasks/$task/state" >/dev/null; fi
  if jq -e '.assigned_worker_id == null' <<<"$record" >/dev/null; then api -X PUT --data "$(jq -nc --arg worker "$worker" '{worker_id:$worker}')" "$base/api/v1/tasks/$task/assignment" >/dev/null; fi
  if [[ $(jq -r .state <<<"$record") =~ ^(draft|ready)$ ]] && jq -e '.running == false' <<<"$profile" >/dev/null; then api -X POST --data '{"rows":40,"columns":120}' "$base/api/v1/workers/$worker/start" | jq '{id,active_session_id}'; fi
fi
jq '{id,running,attention_state,active_session_id}' <<<"$profile"
read_task | jq '{id,state,assigned_worker_id,next_move_owner,dispatch_state}'
api "$base/api/v1/tasks/$task/activity?limit=30" | jq '{truncated,events:[.events[] | {sequence,kind,actor_kind,actor_id,from_state,to_state,occurred_at,note}]}'
api "$base/api/v1/decisions" | jq --arg task "$task" '[.[] | select(.task_id == $task)]'

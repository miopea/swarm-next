#!/usr/bin/env bash
set -euo pipefail

# One fictional task, normal task commands only. Never wake a worker, type into
# a terminal, approve a decision, publish a release, or manually settle the task.
mode=${1:---status}
[[ $mode == --start || $mode == --status ]] || exit 2
base=http://127.0.0.1:8766
worker=01a06eda-bdd1-7a82-928e-cffbee0be6c1
workspace=/home/bschleifer/projects/.swarm-next-dogfood/workflow-fixture
title='Dogfood: Queen-owned staged delivery handoff 20260909'
token=$(sed -n 's/^SWARM_OPERATOR_TOKEN=//p' "$HOME/.config/swarm/swarm.env")
[[ ${#token} -ge 32 ]] || { echo 'Operator credential unavailable'; exit 2; }
api() {
  printf 'header = "Authorization: Bearer %s"\n' "$token" |
    curl --config - --fail --silent --show-error --max-time 30 \
      --header 'Content-Type: application/json' "$@"
}
read_task() {
  { api "$base/api/v1/tasks"; api "$base/api/v1/tasks/settled"; } |
    jq -sce --arg id "$task" '[.[][] | select(.id == $id)] | unique_by(.id) | if length == 1 then .[0] else error("fixture task is missing or duplicated") end'
}
profile=$(api "$base/api/v1/workers" | jq -ce --arg id "$worker" '.[] | select(.id == $id)')
jq -e --arg workspace "$workspace" '.name == "Swarm Dogfood" and .workspace == $workspace and .provider == "claude_code"' <<<"$profile" >/dev/null
board=$(api "$base/api/v1/tasks")
settled=$(api "$base/api/v1/tasks/settled")
matches=$(printf '%s\n' "$board" "$settled" | jq -s --arg title "$title" --arg workspace "$workspace" \
  '[.[][] | select(.title == $title and .workspace == $workspace)] | unique_by(.id)')
[[ $(jq length <<<"$matches") -le 1 ]] || { echo 'More than one fixture exists; refusing to select one.'; exit 2; }
task=$(jq -r '.[0].id // empty' <<<"$matches")
if [[ $mode == --start ]]; then
  jq -e '.running and .attention_state == "resting" and .runtime_error == null' <<<"$profile" >/dev/null || { echo 'Demo worker is not ready; left untouched.'; exit 3; }
  jq -e --arg worker "$worker" --arg task "$task" \
    '[.[] | select(.assigned_worker_id == $worker and .id != $task)] | length == 0' <<<"$board" >/dev/null || { echo 'Demo worker has other unfinished work; left untouched.'; exit 3; }
  if [[ -z $task ]]; then
    description='Fictional staged-delivery acceptance in this disposable repository only. No Swarm/BFG production changes, public services, publishing, release tags, tarballs, pushes, customer messages, provider restarts, or other workers. Use only a new queen-release-handoff-20260909 directory; preserve all existing files. Build a tiny deterministic JSON artifact plus a bounded Node verifier (at most two test workers), and commit only those new fixture source/test files. First finish implementation and record its commit evidence, then enter Review without deploying and without a no-deployment exemption. Ask Queen in one task-linked message to review the evidence, move this task to Awaiting Release, and route the remaining explicitly authorized local delivery back to Swarm Dogfood through normal tools. The worker must not impersonate Queen or self-transition to Awaiting Release. Queen: this task authorizes only installing the fictional artifact into a delivered subdirectory of this same fixture and verifying its exact bytes; this is NOT a Swarm/product release and needs no extra operator approval. Preserve the task assignee and route the second stage after recording the handoff. Worker: wait for that task-scoped handoff before local delivery, then perform the bounded copy, independently verify the delivered bytes and checks, and record truthful whole-task deployment evidence with environment isolated-local-demo and an immutable reference. Do not declare completion before actual delivery. No manual controller intervention is part of this exercise. Report any genuine safety or tooling obstruction accurately rather than claiming a passed handoff.'
    body=$(jq -nc --arg title "$title" --arg workspace "$workspace" --arg description "$description" \
      '{title:$title, workspace:$workspace, description:$description, priority:"normal"}')
    task=$(api -X POST --data "$body" "$base/api/v1/tasks" | jq -er '.id')
    echo "Created fixture task=$task"
  fi
  record=$(read_task)
  jq -e --arg worker "$worker" '.assigned_worker_id == null or .assigned_worker_id == $worker' <<<"$record" >/dev/null || { echo 'Fixture assignment changed; left untouched.'; exit 3; }
  if [[ $(jq -r .state <<<"$record") == draft ]]; then
    record=$(api -X PATCH --data '{"state":"ready"}' "$base/api/v1/tasks/$task/state")
  fi
  if [[ $(jq -r .state <<<"$record") == ready ]] && jq -e '.assigned_worker_id == null' <<<"$record" >/dev/null; then
    api -X PUT --data "$(jq -nc --arg worker "$worker" '{worker_id:$worker}')" "$base/api/v1/tasks/$task/assignment" >/dev/null
  fi
fi
jq '{id,running,attention_state,active_session_id}' <<<"$profile"
[[ -n $task ]] || { echo 'No staged-delivery fixture exists.'; exit 0; }
read_task | jq '{id,state,assigned_worker_id,next_move_owner,dispatch_state,closed_on_evidence,closed_unverifiable}'
api "$base/api/v1/tasks/$task/activity?limit=50" | jq '{truncated,events:[.events[] | {sequence,kind,actor_kind,actor_id,from_state,to_state,occurred_at,note}]}'

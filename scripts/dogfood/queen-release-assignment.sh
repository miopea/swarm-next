#!/usr/bin/env bash
set -euo pipefail

# One operator admission. Queen owns acceptance and the delivery assignment.
# Status is read-only. Never inject input, repair state, grant approval, or ship Swarm.
mode=${1:---status}
[[ $mode == --start || $mode == --status ]] || exit 2
base=http://127.0.0.1:8766
worker=01a06eda-bdd1-7a82-928e-cffbee0be6c1
workspace=/home/bschleifer/projects/.swarm-next-dogfood/workflow-fixture
title='Dogfood: Queen acceptance and separate delivery assignment 20260911'
token=$(sed -n 's/^SWARM_OPERATOR_TOKEN=//p' "$HOME/.config/swarm/swarm.env")
[[ ${#token} -ge 32 ]] || { echo 'Operator credential unavailable'; exit 2; }
api() {
  printf 'header = "Authorization: Bearer %s"\n' "$token" |
    curl --config - --fail --silent --show-error --max-time 30 \
      --header 'Content-Type: application/json' "$@"
}
profile=$(api "$base/api/v1/workers" | jq -ce --arg id "$worker" '.[] | select(.id == $id)')
jq -e --arg workspace "$workspace" '.name == "Swarm Dogfood" and .workspace == $workspace and .provider == "claude_code"' <<<"$profile" >/dev/null
board=$(api "$base/api/v1/tasks")
settled=$(api "$base/api/v1/tasks/settled")
matches=$(printf '%s\n' "$board" "$settled" | jq -s --arg title "$title" --arg workspace "$workspace" '[.[][] | select(.title == $title and .workspace == $workspace)] | unique_by(.id)')
[[ $(jq length <<<"$matches") -le 1 ]] || { echo 'Duplicate fixture; refusing to select one.'; exit 2; }
task=$(jq -r '.[0].id // empty' <<<"$matches")
if [[ $mode == --start && -z $task ]]; then
  jq -e '.running and .attention_state == "resting" and .runtime_error == null' <<<"$profile" >/dev/null || { echo 'Demo worker not ready; untouched.'; exit 3; }
  jq -e --arg worker "$worker" '[.[] | select(.assigned_worker_id == $worker)] | length == 0' <<<"$board" >/dev/null || { echo 'Demo worker has unfinished work; untouched.'; exit 3; }
  [[ -z $(git -C "$workspace" status --porcelain) && ! -e "$workspace/queen-release-assignment-20260911" ]] || { echo 'Fixture workspace not clean/new; untouched.'; exit 3; }
  description='Fictional orchestration acceptance, confined to the new queen-release-assignment-20260911 directory in this disposable repository. Preserve every existing file. No real projects, services, external network, product releases, public publishing, tags, tarballs, pushes, customer messages, credentials, provider restarts, or other workers. Two stages are authorized, but the recorded Queen handoff is a required gate, not optional prose.

BUILD STAGE (this task): create a tiny deterministic JSON artifact, a verifier using built-in Node tests (at most two test workers), and a local delivery command that copies only that artifact into this same fixture directory. Commit only these new source/test files. Do not run delivery yet, create a delivered file, claim a no-deployment exemption, or record deployment evidence. Submit this task for Review with commit/test evidence and one task-linked message to Queen. Ask for acceptance and a separate delivery assignment. After submission wait for that assignment; do not finish delivery merely because a review reply requests it.

QUEEN HANDOFF: inspect the build evidence. If acceptable, move this original build task from Review to Awaiting Release. Then create exactly one separate Ready delivery task in this same workspace, assigned to Swarm Dogfood, carrying this original task full ID, source commit, confined delivery scope, and this gate. No completion prerequisite from delivery to the unshipped parent: that would deadlock shipment. Preserve the parent assignee. This task already authorizes the confined local fixture delivery, not a Swarm/product release; no further operator permission is needed. Do not use a returned-review request instead of recording acceptance and the separate assignment. If the build genuinely needs revision, return it with the specific issue before acceptance.

DELIVERY STAGE (Queen-created task only): before any delivery write, read the original build task through Swarm and verify it is Awaiting Release, assigned to Swarm Dogfood, and that Queen created this delivery assignment with the same exact parent ID and source commit. If any gate is absent, stop and report the exact mismatch to Queen; do not self-transition the parent or fabricate a receipt. When valid, execute the committed confined delivery command, verify delivered bytes and tests, and record truthful deployment evidence for the original build task using environment isolated-local-demo and an immutable reference. Let the system settle the parent on evidence. Submit the delivery task with its actual evidence too. Report any genuine tool/authority obstruction instead of replacing the requested lifecycle. No manual controller repair is part of this acceptance.'
  body=$(jq -nc --arg title "$title" --arg workspace "$workspace" --arg description "$description" '{title:$title,workspace:$workspace,description:$description,priority:"normal"}')
  task=$(api -X POST --data "$body" "$base/api/v1/tasks" | jq -er '.id')
  echo "Created fixture task=$task"
fi
[[ -n $task ]] || { echo 'No fixture exists.'; exit 0; }
read_task() {
  { api "$base/api/v1/tasks"; api "$base/api/v1/tasks/settled"; } |
    jq -sce --arg id "$task" '[.[][] | select(.id == $id)] | unique_by(.id) | if length == 1 then .[0] else error("fixture missing or duplicated") end'
}
if [[ $mode == --start ]]; then
  record=$(read_task)
  jq -e --arg worker "$worker" '.assigned_worker_id == null or .assigned_worker_id == $worker' <<<"$record" >/dev/null || { echo 'Assignment changed; untouched.'; exit 3; }
  if [[ $(jq -r .state <<<"$record") == draft ]]; then
    record=$(api -X PATCH --data '{"state":"ready"}' "$base/api/v1/tasks/$task/state")
  fi
  if [[ $(jq -r .state <<<"$record") == ready ]] && jq -e '.assigned_worker_id == null' <<<"$record" >/dev/null; then
    api -X PUT --data "$(jq -nc --arg worker "$worker" '{worker_id:$worker}')" "$base/api/v1/tasks/$task/assignment" >/dev/null
  fi
fi
jq '{id,running,attention_state,active_session_id}' <<<"$profile"
read_task | jq '{id,state,assigned_worker_id,next_move_owner,dispatch_state,closed_on_evidence}'
api "$base/api/v1/tasks/$task/activity?limit=50" | jq '{truncated,events:[.events[] | {sequence,kind,actor_kind,actor_id,from_state,to_state,occurred_at,note}]}'

#!/usr/bin/env bash
set -euo pipefail
# Isolated native-provider background observation. Never writes PTY input,
# changes a real task, restarts a service or manually completes the fixture.
background_mode=${1:---status}
[[ $background_mode == --start || $background_mode == --status || $background_mode == --release ]] || exit 2
background_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
source "$background_root/queen-release-assignment.sh" --status >/dev/null
background_title='Dogfood: preserve a native background job 20260911'
background_matches=$(
  { api "$base/api/v1/tasks"; api "$base/api/v1/tasks/settled"; } |
    jq -sc --arg title "$background_title" --arg workspace "$workspace" '[.[][] | select(.title == $title and .workspace == $workspace)] | unique_by(.id)'
)
[[ $(jq length <<<"$background_matches") -le 1 ]] || { echo 'Duplicate fixture; untouched.'; exit 2; }
background_task=$(jq -r '.[0].id // empty' <<<"$background_matches")
if [[ $background_mode == --start && -z $background_task ]]; then
  jq -e '.running and .provider_activity == "resting" and .engaged_device_id == null and .background_work == false' <<<"$profile" >/dev/null
  jq -e --arg worker "$worker" '[.[] | select(.assigned_worker_id == $worker)] | length == 0' <<<"$board" >/dev/null
  background_dir=$(mktemp -d /tmp/swarm-background-gate.XXXXXXXX)
  chmod 700 "$background_dir"
  background_fifo="$background_dir/release"
  mkfifo -m 600 "$background_fifo"
  background_description="FICTIONAL NATIVE BACKGROUND SAFETY ACCEPTANCE. Only this existing demo workspace and the one controller-owned FIFO below are in scope.
BACKGROUND_GATE_FIFO=$background_fifo
First mark this task Active. Start exactly one Bash tool job using its native run_in_background=true option: timeout 240 bash -c 'IFS= read -r response < \"\$1\"; test \"\$response\" = release' bash '$background_fifo'. This waits for a controller event, not for an arbitrary delay; timeout is a hard safety bound. Report that the native background job is running, then END YOUR TURN while leaving this task Active. Do not poll, answer a decision, send Queen a message, or call TaskOutput with blocking=true yet. This tests the genuine provider resting-with-background display. Queen must not treat this known background job as an abandoned task or inject a continuation while it is running.
The controller will release only this FIFO after observing the live state. When the provider's normal background completion notification arrives, check its actual exit status. Exit zero means the controller released it; exit124 means the test timed out and MUST be reported as a failure, not success. Then run the twelve existing queen-release-assignment-20260911/deliver.test.mjs tests with Node and test-concurrency=2, verify the existing delivered artifact SHA256 is 9540c83bf8c3ece4bcc0f6b8d4e6daafb74d30bea682fb0d6d801fe3fd51bf8e, and submit truthful no-deployment investigation evidence and an explicitly empty commit list through the ordinary Review path. Do not manually Complete. No source edits, commits, delivery writes, other tasks/workers, network, credentials, releases, provider restarts or services. Do not remove the FIFO; the controller owns cleanup. No further permission is needed for this confined test."
  background_body=$(jq -nc --arg title "$background_title" --arg workspace "$workspace" --arg description "$background_description" '{title:$title,workspace:$workspace,description:$description,priority:"normal"}')
  background_task=$(api -X POST --data "$background_body" "$base/api/v1/tasks" | jq -er '.id')
  api -X PATCH --data '{"state":"ready"}' "$base/api/v1/tasks/$background_task/state" >/dev/null
  api -X PUT --data "$(jq -nc --arg worker "$worker" '{worker_id:$worker}')" "$base/api/v1/tasks/$background_task/assignment" >/dev/null
  echo "Created task=$background_task; FIFO=$background_fifo"
fi
[[ -n $background_task ]] || { echo 'No background fixture exists.'; exit 0; }
if [[ $background_mode == --release ]]; then
  background_fifo=$(jq -r '.[0].description' <<<"$background_matches" | sed -n 's/^BACKGROUND_GATE_FIFO=//p')
  # Require the caller to name the exact FIFO returned by --start. A later
  # editable task description alone cannot authorize a different write target.
  [[ ${2:-} == "$background_fifo" ]] || { echo 'Name the exact controller-created FIFO as the second argument.'; exit 2; }
  jq -e '.[0].state == "active"' <<<"$background_matches" >/dev/null
  [[ $background_fifo =~ ^/tmp/swarm-background-gate\.[a-zA-Z0-9]{8}/release$ && -p $background_fifo && ! -L $background_fifo ]] || { echo 'Unexpected FIFO; untouched.'; exit 2; }
  [[ $(realpath "$background_fifo") == "$background_fifo" ]] || exit 2
  timeout 5 bash -c 'printf "release\n" > "$1"' bash "$background_fifo"
  echo 'Released only the fictional background FIFO.'
fi
jq '{id,running,active_session_id,engaged_device_id,attention_state,provider_activity,background_work}' <<<"$profile"
{ api "$base/api/v1/tasks"; api "$base/api/v1/tasks/settled"; } |
  jq -sc --arg id "$background_task" '[.[][] | select(.id == $id) | {id,state,next_move_owner,assigned_worker_id,dispatch_state}] | unique_by(.id)'
api "$base/api/v1/orchestration/coordinator?include_review=false" |
  jq --arg id "$background_task" '{held_briefings:[.held_briefings[]? | select(.task_id == $id)],recovery:(if .recovery == null then null else {items:[.recovery.items[] | select(.task_id == $id)],truncated:.recovery.truncated} end)}'
api "$base/api/v1/tasks/$background_task/activity?limit=12" |
  jq '{truncated,events:[.events[] | {sequence,kind,actor_kind,from_state,to_state,occurred_at,note}]}'

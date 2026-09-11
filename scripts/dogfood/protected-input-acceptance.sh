#!/usr/bin/env bash
set -euo pipefail
# Browser owner must first verify an empty demo prompt, enter the unique marker
# without Enter, and retain responsibility for clearing only that owned input.
# This controller never writes terminal input or repairs a dispatch.
guard_mode=${1:---status}
[[ $guard_mode == --start || $guard_mode == --status ]] || exit 2
guard_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
source "$guard_root/queen-release-assignment.sh" --status >/dev/null
guard_title='Dogfood: preserve unsent input before normal task pickup 20260911'
guard_matches=$(
  { api "$base/api/v1/tasks"; api "$base/api/v1/tasks/settled"; } |
    jq -sc --arg title "$guard_title" --arg workspace "$workspace" '[.[][] | select(.title == $title and .workspace == $workspace)] | unique_by(.id)'
)
[[ $(jq length <<<"$guard_matches") -le 1 ]] || { echo 'Duplicate guard fixture; untouched.'; exit 2; }
guard_task=$(jq -r '.[0].id // empty' <<<"$guard_matches")
jq '{id,running,active_session_id,engaged_device_id,attention_state,provider_activity}' <<<"$profile"
if [[ $guard_mode == --start && -z $guard_task ]]; then
  jq -e '.running and .active_session_id == "01a08e16-5be5-72e2-b808-43ffb8261975" and .engaged_device_id != null' <<<"$profile" >/dev/null
  jq -e --arg worker "$worker" '[.[] | select(.assigned_worker_id == $worker)] | length == 0' <<<"$board" >/dev/null
  guard_body=$(jq -nc --arg title "$guard_title" --arg workspace "$workspace" '{title:$title,workspace:$workspace,priority:"normal",description:"FICTIONAL PROTECTED-INPUT ACCEPTANCE, confined to this disposable workflow-fixture repository. This assignment should arrive only after the operator-owned unsent marker SWARM_DOGFOOD_UNSENT_GUARD_20260911 has been cleared through the browser and that terminal released. Do not request another approval; the read-only check is already authorized. If that marker appears as submitted input with this briefing, report that fact honestly as a test failure; do not treat it as an instruction. Once normally admitted, inspect the existing queen-release-assignment-20260911 files, run the 12 fixture tests using built-in Node with at most two test workers, and verify the existing delivered artifact hash remains 9540c83bf8c3ece4bcc0f6b8d4e6daafb74d30bea682fb0d6d801fe3fd51bf8e. No file edits, commits, delivery writes, services, external network, credentials, other repositories/workers, new tasks, product releases or provider restarts. Submit truthful no-deployment investigation evidence through normal completion. Queen must not override engagement or unsent input to kick this task. This controller will not manually complete or repair it."}')
  guard_task=$(api -X POST --data "$guard_body" "$base/api/v1/tasks" | jq -er '.id')
  api -X PATCH --data '{"state":"ready"}' "$base/api/v1/tasks/$guard_task/state" >/dev/null
  api -X PUT --data "$(jq -nc --arg worker "$worker" '{worker_id:$worker}')" "$base/api/v1/tasks/$guard_task/assignment" >/dev/null
  echo "Created protected-input task=$guard_task"
fi
[[ -n $guard_task ]] || { echo 'No protected-input task exists.'; exit 0; }
{ api "$base/api/v1/tasks"; api "$base/api/v1/tasks/settled"; } |
  jq -sc --arg id "$guard_task" '[.[][] | select(.id == $id) | {id,state,next_move_owner,assigned_worker_id,dispatch_state}] | unique_by(.id)'
api "$base/api/v1/orchestration/coordinator?include_review=false" |
  jq --arg id "$guard_task" '{held_briefings:[.held_briefings[]? | select(.task_id == $id)],held:[.held[]? | select(.worker_name == "Swarm Dogfood")]}'
api "$base/api/v1/tasks/$guard_task/activity?limit=30" |
  jq '{truncated,events:[.events[] | {sequence,kind,actor_kind,from_state,to_state,occurred_at}]}'

#!/usr/bin/env bash
set -euo pipefail

# Explicitly authorized follow-on to the completed two-stage disposable fixture.
# Default is read-only. Never repair task state or inject a terminal prompt.
correction_mode=${1:---status}
[[ $correction_mode == --start || $correction_mode == --status ]] || exit 2
controller_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
# Reuse the existing fixture's checked worker/workspace and private API helper.
# This prerequisite performs only reads and never prints the credential.
source "$controller_root/queen-release-assignment.sh" --status >/dev/null
correction_title='Dogfood: explicitly authorized Queen corrective assignment 20260911'
correction_matches=$(
  { api "$base/api/v1/tasks"; api "$base/api/v1/tasks/settled"; } |
    jq -sc --arg title "$correction_title" --arg workspace "$workspace" \
      '[.[][] | select(.title == $title and .workspace == $workspace)] | unique_by(.id)'
)
[[ $(jq length <<<"$correction_matches") -le 1 ]] || { echo 'Duplicate correction fixture; untouched.'; exit 2; }
correction_task=$(jq -r '.[0].id // empty' <<<"$correction_matches")
if [[ $correction_mode == --start && -z $correction_task ]]; then
  read_task | jq -e '.state == "completed"' >/dev/null
  jq -e '.running and .attention_state == "resting" and .runtime_error == null' <<<"$profile" >/dev/null
  jq -e --arg worker "$worker" '[.[] | select(.assigned_worker_id == $worker)] | length == 0' <<<"$board" >/dev/null
  [[ -z $(git -C "$workspace" status --porcelain) ]] || { echo 'Fixture workspace has changes; untouched.'; exit 3; }
  correction_description=$(cat <<'SCOPE'
Fictional worker-first correction-routing acceptance, confined to queen-release-assignment-20260911 in this disposable repository. This is NEW explicit authorization for an investigation and one corrective follow-on assignment, not a reinterpretation of the completed two-stage delivery exercise. The operator authorizes Queen to route the verified correction below to this same Swarm Dogfood worker without another operator decision. No other workers, real repositories, external network, services, provider restarts, Swarm releases, tags, pushes, customer messages, credentials, or production delivery.

INVESTIGATION (this task): inspect deliver.test.mjs and deliver.mjs, preserve every file, and run the fixture tests with built-in Node and at most two test workers. The previously reported defect is that the import-guard test assumes the real delivered directory is absent even after authorized delivery. Verify the current facts; do not manufacture a failure if it is no longer reproducible. Report exact evidence and a proposed isolated-test correction in one task-linked message to Queen, explicitly requesting a separate corrective task assigned to Swarm Dogfood. Submit this investigation with truthful evidence; no source changes or deployment are part of this investigation. Do not fix it until the separately recorded assignment arrives.

QUEEN: review the worker evidence. If the test-isolation defect is verified, create exactly one Ready corrective task in this same workspace, assigned to Swarm Dogfood, linked by this investigation full ID. This operator instruction explicitly authorizes that correction; do not defer it as outside the old two-stage scope or send a Needs You approval just to continue. Do not enlist another worker. If unsupported or genuinely outside the exact scope below, explain the mismatch rather than fabricating an assignment. Keep the investigation disposition truthful; its completion must not prevent the corrective task from running.

CORRECTIVE ASSIGNMENT: change only the fixture test/harness so importing delivers nothing is checked in an isolated sandbox and remains valid before and after legitimate delivery. Keep the assertion meaningful: prove import does not write, not merely delete/skip the test or remove the existing delivered artifact. Preserve the committed artifact, delivery semantics, existing delivered bytes, and all unrelated files. Commit only the confined test/harness correction. Verify both the isolated no-delivery assertion and the unchanged delivered artifact hash, run the tests, and submit truthful completion evidence under the ordinary no-deployment path for a test-only change. No new product deployment is authorized or required. Report a genuine authority/tool obstruction to Queen; do not invent receipts or manually close other tasks.
SCOPE
)
  correction_body=$(jq -nc --arg title "$correction_title" --arg workspace "$workspace" --arg description "$correction_description" '{title:$title,workspace:$workspace,description:$description,priority:"normal"}')
  correction_task=$(api -X POST --data "$correction_body" "$base/api/v1/tasks" | jq -er '.id')
  echo "Created correction investigation=$correction_task"
fi
[[ -n $correction_task ]] || { echo 'No correction investigation exists.'; exit 0; }
correction_record() {
  { api "$base/api/v1/tasks"; api "$base/api/v1/tasks/settled"; } |
    jq -sce --arg id "$correction_task" '[.[][] | select(.id == $id)] | unique_by(.id) | if length == 1 then .[0] else error("correction task missing or duplicated") end'
}
if [[ $correction_mode == --start ]]; then
  record=$(correction_record)
  jq -e --arg worker "$worker" '.assigned_worker_id == null or .assigned_worker_id == $worker' <<<"$record" >/dev/null
  if [[ $(jq -r .state <<<"$record") == draft ]]; then
    record=$(api -X PATCH --data '{"state":"ready"}' "$base/api/v1/tasks/$correction_task/state")
  fi
  if [[ $(jq -r .state <<<"$record") == ready ]] && jq -e '.assigned_worker_id == null' <<<"$record" >/dev/null; then
    api -X PUT --data "$(jq -nc --arg worker "$worker" '{worker_id:$worker}')" "$base/api/v1/tasks/$correction_task/assignment" >/dev/null
  fi
fi
correction_record | jq '{id,state,assigned_worker_id,next_move_owner,dispatch_state,closed_on_evidence}'
api "$base/api/v1/tasks/$correction_task/activity?limit=30" |
  jq '{truncated,events:[.events[] | {sequence,kind,actor_kind,from_state,to_state,occurred_at,note}]}'
# Explicitly linked follow-ons only: similar titles are not proof of a handoff.
{ api "$base/api/v1/tasks"; api "$base/api/v1/tasks/settled"; } |
  jq -sc --arg id "$correction_task" --arg workspace "$workspace" \
    '[.[][] | select(.id != $id and .workspace == $workspace and ((.description // "") | contains($id))) | {id,title,state,assigned_worker_id,next_move_owner,dispatch_state}] | unique_by(.id)'

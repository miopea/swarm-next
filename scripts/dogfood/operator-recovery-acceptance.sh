#!/usr/bin/env bash
set -euo pipefail
# One fictional task, normal decision delivery, no direct terminal input.
# --setup refuses duplicates. Resolve only the exact resulting fictional decision
# after inspecting its contents; this script never answers real operator requests.
base=http://127.0.0.1:8766
label=${SWARM_DOGFOOD_RUN_LABEL:-207bddbc}
[[ $label =~ ^[a-zA-Z0-9._-]{1,64}$ ]] || { echo 'Invalid fictional run label'; exit 2; }
title="Dogfood $label: decision-to-worker recovery"
worker=01a06eda-bdd1-7a82-928e-cffbee0be6c1
workspace=/home/bschleifer/projects/.swarm-next-dogfood/workflow-fixture
token=$(sed -n 's/^SWARM_OPERATOR_TOKEN=//p' "$HOME/.config/swarm/swarm.env")
[[ ${#token} -ge 32 ]] || exit 2
api() {
  printf 'header = "Authorization: Bearer %s"\n' "$token" |
    curl --config - --fail --silent --show-error --max-time 30 "$@"
}
scenario() {
  { api "$base/api/v1/tasks"; api "$base/api/v1/tasks/settled"; } |
    jq -s --arg title "$title" '[.[][] | select(.title==$title)] | unique_by(.id)'
}
if [[ ${1:-} == --status ]]; then
  scenario | jq '[.[] | {id,title,state,next_move_owner,assigned_worker_id,dispatch_state,closed_on_evidence}]'
  exit 0
fi
[[ ${1:-} == --setup ]] || { echo 'Use --setup or --status'; exit 2; }
existing=$(scenario)
[[ $(jq length <<<"$existing") == 0 ]] || { echo 'Scenario already exists; inspect --status'; exit 1; }
api "$base/api/v1/workers" | jq -e --arg worker "$worker" --arg workspace "$workspace" \
  '[.[] | select(.id==$worker and .workspace==$workspace and .active_session_id!=null and .provider_activity=="resting" and .background_work==false)] | length==1' >/dev/null
api "$base/api/v1/tasks" | jq -e --arg worker "$worker" \
  '[.[] | select(.assigned_worker_id==$worker and (.state=="active" or .state=="ready" or .state=="review"))] | length==0' >/dev/null
body=$(jq -nc --arg title "$title" --arg workspace "$workspace" '{title:$title,workspace:$workspace,description:"FICTIONAL SWARM ACCEPTANCE ONLY. First use swarm_request_decision linked to this exact task to ask: May this fictional test run the demo repository existing read-only checks? Offer exactly Run read-only checks and Hold this test, recommending Run read-only checks. Clearly label the question DOGFOOD. This intentional decision exercises recovery, not a real missing authorization. Stop this turn after creating the decision; do not poll for its answer. When the normal Swarm decision outcome arrives, read the saved resolution. Only Run read-only checks permits continuing: run the existing local Node tests in this workspace, report exact pass/fail/test count, empty task commit list and truthful no-deployment investigation evidence, then submit Review for routine automatic settlement. Hold this test means remain waiting without retries. Do not edit files, commit, deploy, access outside this fixture, contact external services or create additional tasks/decisions. A zero-test result is not a passing verification. The test controller will answer this one fictional decision through Swarm, with no direct terminal input."}')
task=$(api -X POST -H 'Content-Type: application/json' --data "$body" "$base/api/v1/tasks" | jq -er '.id')
echo "TASK=$task"
api -X PATCH -H 'Content-Type: application/json' --data '{"state":"ready","note":"Isolated decision recovery acceptance through normal guarded dispatch"}' "$base/api/v1/tasks/$task/state" >/dev/null
api -X PUT -H 'Content-Type: application/json' --data "$(jq -nc --arg worker "$worker" '{worker_id:$worker}')" "$base/api/v1/tasks/$task/assignment" >/dev/null
scenario | jq '[.[] | {id,title,state,next_move_owner,assigned_worker_id,dispatch_state}]'

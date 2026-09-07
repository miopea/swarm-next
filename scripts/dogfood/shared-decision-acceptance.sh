#!/usr/bin/env bash
set -euo pipefail
# Fictional work only. No raw terminal input, backdating, or forced completion.
base=http://127.0.0.1:8766
label=${SWARM_DOGFOOD_RUN_LABEL:-5663a1f7}
[[ $label =~ ^[a-zA-Z0-9._-]{1,64}$ ]] || exit 2
prefix="Dogfood $label: shared decision"
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
    jq -s --arg prefix "$prefix" '[.[][] | select(.title==($prefix+" source") or .title==($prefix+" consumer"))] | unique_by(.id)'
}
case ${1:-} in
  --status)
    tasks=$(scenario)
    jq '[.[] | {id,title,state,next_move_owner,assigned_worker_id,dispatch_state}]' <<<"$tasks"
    api "$base/api/v1/decisions" | jq --argjson tasks "$tasks" '[.[] | select(.task_id as $id | any($tasks[]; .id==$id)) | {id,title,state,task_id,linked_tasks}]'
    ;;
  --setup)
    [[ $(scenario | jq length) == 0 ]] || { echo 'Scenario exists; inspect --status instead of duplicating it'; exit 1; }
    api "$base/api/v1/workers" | jq -e --arg worker "$worker" --arg workspace "$workspace" \
      '[.[] | select(.id==$worker and .workspace==$workspace and .running==true and .provider_activity=="resting" and .background_work==false and .engaged_device_id==null)] | length==1' >/dev/null
    api "$base/api/v1/tasks" | jq -e --arg worker "$worker" '[.[] | select(.assigned_worker_id==$worker)] | length==0' >/dev/null
    consumer_body=$(jq -nc --arg title "$prefix consumer" --arg workspace "$workspace" '{title:$title,workspace:$workspace,description:"FICTIONAL SHARED-DECISION ACCEPTANCE, demo repository only. Queen: wait for the companion source task to raise its one fictional input decision. Verify that pending decision, read this task review evidence, and explicitly link this task with swarm_set_task_decision_link. Then make this task Ready and assign it to Swarm Dogfood; the pending shared gate must hold its briefing. Do not create a duplicate question, bypass the gate, or treat its answer as command permission. After the gate clears, the worker should inspect only this demo repository, run its documented local Node tests, report the actual test count/results and truthful no-deployment investigation evidence, then submit Review for ordinary settlement. No file edits, commits, deployments, external services, or work in other repositories. This ticket is already authorized fictional test work, not a request for another operator approval."}')
    consumer=$(api -X POST -H 'Content-Type: application/json' --data "$consumer_body" "$base/api/v1/tasks" | jq -er '.id')
    source_body=$(jq -nc --arg title "$prefix source" --arg workspace "$workspace" --arg consumer "$consumer" --arg question "$prefix input" '{title:$title,workspace:$workspace,description:("FICTIONAL SHARED-DECISION ACCEPTANCE, demo repository only. Start this task normally, then use swarm_request_decision to ask exactly one INPUT question linked to THIS task, titled "+$question+", with the single allowed action Use fictional sample A. It asks for test input, not command authorization; do not set a requested command. Record this task Blocked on that pending decision. Ask Queen through normal task messaging to verify the same gate and use swarm_set_task_decision_link on companion task "+$consumer+", make that companion Ready, and assign it to this same Swarm Dogfood worker. Queen owns that routing; do not modify or assign the companion yourself. Do not create another question or continue past the pending gate. When the fictional answer arrives, recover through normal guarded task transitions and run this repository documented local Node tests, report actual test count/results and truthful no-deployment investigation evidence, then submit Review for ordinary settlement. No file edits, commits, deployment, external services, new tasks, or operations outside this fixture. Do not claim the whole acceptance passed; the controller verifies both tasks and the single decision.")}')
    source=$(api -X POST -H 'Content-Type: application/json' --data "$source_body" "$base/api/v1/tasks" | jq -er '.id')
    api -X PATCH -H 'Content-Type: application/json' --data '{"state":"ready","note":"Start the authorized fictional shared-decision scenario"}' "$base/api/v1/tasks/$source/state" >/dev/null
    api -X PUT -H 'Content-Type: application/json' --data "$(jq -nc --arg worker "$worker" '{worker_id:$worker}')" "$base/api/v1/tasks/$source/assignment" >/dev/null
    scenario | jq '[.[] | {id,title,state,next_move_owner,assigned_worker_id}]'
    ;;
  --resolve)
    tasks=$(scenario)
    [[ $(jq length <<<"$tasks") == 2 ]] || exit 1
    jq -e 'all(.[]; .next_move_owner=="operator")' <<<"$tasks" >/dev/null
    source=$(jq -er --arg title "$prefix source" '.[] | select(.title==$title) | .id' <<<"$tasks")
    consumer=$(jq -er --arg title "$prefix consumer" '.[] | select(.title==$title) | .id' <<<"$tasks")
    questions=$(api "$base/api/v1/decisions" | jq --arg source "$source" '[.[] | select(.task_id==$source and .state=="pending")]')
    jq -e --arg consumer "$consumer" --arg title "$prefix input" 'length==1 and .[0].title==$title and .[0].allowed_actions==["Use fictional sample A"] and (.[0].requested_command==null) and any(.[0].linked_tasks[]; .task_id==$consumer)' <<<"$questions" >/dev/null
    question=$(jq -er '.[0].id' <<<"$questions")
    # The only simulated operator action; subsequent task recovery must be real.
    api -X PATCH -H 'Content-Type: application/json' --data '{"action":"Use fictional sample A","note":"Fictional test input only. No additional command permission.","surface":"dogfood-shared-decision"}' "$base/api/v1/decisions/$question/resolution"
    ;;
  *) echo 'Use --setup, --status, or --resolve'; exit 2 ;;
esac

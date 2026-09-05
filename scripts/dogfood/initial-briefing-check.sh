#!/usr/bin/env bash
set -euo pipefail

# Explicitly limited to the existing disposable dogfood worker. Run on the Hive.
# Credentials travel through curl stdin, never arguments or test output.
base=http://127.0.0.1:8766
workspace=/home/bschleifer/projects/.swarm-next-dogfood/workflow-fixture
worker=01a06eda-bdd1-7a82-928e-cffbee0be6c1
resume_task=${1:-}
token=$(sed -n 's/^SWARM_OPERATOR_TOKEN=//p' "$HOME/.config/swarm/swarm.env")
[[ ${#token} -ge 32 ]] || { echo 'Operator credential unavailable'; exit 2; }
api() {
  printf 'header = "Authorization: Bearer %s"\n' "$token" |
    curl --config - --fail --silent --show-error --max-time 30 \
      --header 'Content-Type: application/json' "$@"
}
profile=$(api "$base/api/v1/workers" | jq -ce --arg id "$worker" '.[] | select(.id == $id)')
jq -e --arg workspace "$workspace" \
  '.name == "Swarm Dogfood" and .workspace == $workspace and .provider == "claude_code"' \
  <<<"$profile" >/dev/null
api "$base/api/v1/tasks" | jq -e --arg id "$worker" --arg resume "$resume_task" \
  '[.[] | select(.assigned_worker_id == $id and .state == "active" and .id != $resume)] | length == 0' >/dev/null
if [[ $resume_task == --stop ]]; then
  api -X DELETE "$base/api/v1/workers/$worker/session" >/dev/null
  echo 'Idle demo worker stopped; tasks and conversations retained.'
  exit 0
fi
if ! jq -e '.running' <<<"$profile" >/dev/null; then
  api -X POST --data '{"rows":40,"columns":120}' "$base/api/v1/workers/$worker/start" >/dev/null
fi
run=$(date -u +%Y%m%dT%H%M%SZ)
for step in 1 2; do
  title="Dogfood initial-briefing timing $run step $step"
  body=$(jq -nc --arg title "$title" --arg workspace "$workspace" --arg run "$run" --arg step "$step" \
    '{title:$title, workspace:$workspace, priority:"normal", operator_instruction:"Disposable documentation-only timing check; do not deploy or change other projects.", description:("Create documentation file dogfood-" + $run + "-" + $step + ".md containing one sentence confirming this task was received. Commit only that file. Record its commit using Swarm tools and report Review with a truthful documentation-only no-deployment claim so routine settlement can close it. No other changes or questions are needed.")}')
  if [[ $step == 1 && -n $resume_task ]]; then
    task=$resume_task
    echo "resuming observation task=$task"
    started=$(date +%s)
  else
  task=$(api -X POST --data "$body" "$base/api/v1/tasks" | jq -er '.id')
  echo "created task=$task step=$step"
  api -X PATCH --data '{"state":"ready"}' "$base/api/v1/tasks/$task/state" >/dev/null
  started=$(date +%s)
  api -X PUT --data "$(jq -nc --arg worker "$worker" '{worker_id:$worker}')" "$base/api/v1/tasks/$task/assignment" >/dev/null
  fi
  delivered=0
  finished=0
  deadline=$((started + 240))
  while (( $(date +%s) < deadline )); do
    record=$(api "$base/api/v1/tasks" | jq -c --arg id "$task" '.[] | select(.id == $id)')
    if [[ -z $record ]]; then
      record=$(api "$base/api/v1/tasks/settled" | jq -ce --arg id "$task" '.[] | select(.id == $id)')
    fi
    jq -e --arg workspace "$workspace" --arg worker "$worker" \
      '.workspace == $workspace and .assigned_worker_id == $worker' <<<"$record" >/dev/null
    state=$(jq -r '.state' <<<"$record")
    dispatch=$(jq -r '.dispatch_state' <<<"$record")
    if [[ $dispatch == delivered && $delivered == 0 ]]; then
      delivered=$(date +%s)
      echo "delivered task=$task observation_seconds=$((delivered - started)) (use durable timestamps if resumed)"
    fi
    if [[ $state == completed ]]; then
      echo "completed task=$task elapsed_seconds=$(($(date +%s) - started))"
      finished=1
      break
    fi
    if [[ $state == blocked || $dispatch == uncertain ]]; then
      echo "attention task=$task state=$state dispatch=$dispatch"
      exit 1
    fi
    sleep 5
  done
  if (( finished == 0 )); then
    echo "timeout task=$task; retained for inspection, worker not interrupted"
    exit 1
  fi
done
echo 'Both demo tasks completed; inspect durable dispatch timestamps for the cooldown comparison.'
exit 0

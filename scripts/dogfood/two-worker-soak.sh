#!/usr/bin/env bash
set -euo pipefail

# Exercises two real provider sessions while sampling the resource owners that
# must remain bounded. The operator token is supplied through a mode-0600 curl
# config so it never appears in process arguments or test output.

base_url="${SWARM_SOAK_BASE_URL:-http://127.0.0.1:8766}"
duration_seconds="${SWARM_SOAK_DURATION_SECONDS:-86400}"
sample_seconds="${SWARM_SOAK_SAMPLE_SECONDS:-30}"
restart_every_seconds="${SWARM_SOAK_RESTART_API_EVERY_SECONDS:-900}"
workspace_a="${SWARM_SOAK_WORKSPACE_A:-}"
workspace_b="${SWARM_SOAK_WORKSPACE_B:-${workspace_a}}"
report_dir="${SWARM_SOAK_REPORT_DIR:-${HOME}/.local/state/swarm-next/soak}"

if [[ -z "${SWARM_OPERATOR_TOKEN:-}" || -z "${workspace_a}" ]]; then
  echo "SWARM_OPERATOR_TOKEN and SWARM_SOAK_WORKSPACE_A are required" >&2
  exit 2
fi

for value in "${duration_seconds}" "${sample_seconds}" "${restart_every_seconds}"; do
  if [[ ! "${value}" =~ ^[0-9]+$ ]]; then
    echo "soak durations must be whole seconds" >&2
    exit 2
  fi
done
if (( duration_seconds < 60 || sample_seconds < 1 )); then
  echo "duration must be at least 60 seconds and sample interval at least 1 second" >&2
  exit 2
fi

mkdir -p "${report_dir}"
chmod 700 "${report_dir}"
run_id="$(date -u +%Y%m%dT%H%M%SZ)"
samples_file="${report_dir}/${run_id}-samples.csv"
summary_file="${report_dir}/${run_id}-summary.json"
curl_config="$(mktemp "${TMPDIR:-/tmp}/swarm-next-soak.XXXXXX")"
chmod 600 "${curl_config}"
printf 'header = "Authorization: Bearer %s"\n' "${SWARM_OPERATOR_TOKEN}" >"${curl_config}"

session_a=""
session_b=""
wait_api() {
  local attempts=0
  while ! curl --fail --silent --show-error --max-time 1 "${base_url}/health" >/dev/null; do
    attempts=$((attempts + 1))
    if (( attempts >= 50 )); then
      return 1
    fi
    sleep 0.1
  done
}

cleanup() {
  if [[ "${SWARM_SOAK_KEEP_WORKERS:-0}" != "1" ]]; then
    wait_api || true
    for session_id in "${session_a}" "${session_b}"; do
      if [[ -n "${session_id}" ]]; then
        curl --silent --show-error --config "${curl_config}" --request DELETE \
          "${base_url}/api/v1/terminal/sessions/${session_id}" >/dev/null || {
            wait_api || true
            curl --silent --show-error --config "${curl_config}" --request DELETE \
              "${base_url}/api/v1/terminal/sessions/${session_id}" >/dev/null || true
          }
      fi
    done
  fi
  rm -f "${curl_config}"
}
trap cleanup EXIT INT TERM

api_json() {
  curl --fail --silent --show-error --config "${curl_config}" \
    --header 'Content-Type: application/json' "$@"
}

start_worker() {
  local workspace="$1"
  api_json --request POST --data "$(printf '{\"workspace\":%s,\"rows\":40,\"columns\":120}' \
    "$(jq -Rn --arg value "${workspace}" '$value')")" \
    "${base_url}/api/v1/terminal/sessions" | jq -er '.session_id'
}

session_a="$(start_worker "${workspace_a}")"
session_b="$(start_worker "${workspace_b}")"
printf 'timestamp_utc,elapsed_seconds,api_memory_bytes,api_tasks,terminal_host_memory_bytes,terminal_host_tasks,running_sessions,retained_sessions,history_bytes,dropped_history_bytes\n' >"${samples_file}"

started_at="$(date +%s)"
next_restart=$((started_at + restart_every_seconds))
deadline=$((started_at + duration_seconds))
sample_count=0

while (( $(date +%s) < deadline )); do
  now="$(date +%s)"
  if (( restart_every_seconds > 0 && now >= next_restart )); then
    systemctl --user restart swarm-next-api.service
    wait_api
    next_restart=$((now + restart_every_seconds))
  fi

  sessions="$(api_json "${base_url}/api/v1/terminal/sessions")"
  for session_id in "${session_a}" "${session_b}"; do
    jq -e --arg id "${session_id}" '.sessions[] | select(.session_id == $id and .running == true)' \
      <<<"${sessions}" >/dev/null
  done

  host_status="$(api_json "${base_url}/api/v1/runtime/terminal-host")"
  history="$(api_json "${base_url}/api/v1/terminal/history/diagnostics")"
  running_sessions="$(jq -er '.status.running_sessions | numbers' <<<"${host_status}")"
  retained_sessions="$(jq -er '.status.retained_sessions | numbers' <<<"${host_status}")"
  history_bytes="$(jq -er '.diagnostics.retained_bytes | numbers' <<<"${history}")"
  dropped_history_bytes="$(jq -er '.diagnostics.dropped_bytes | numbers' <<<"${history}")"
  printf '%s,%s,%s,%s,%s,%s,%s,%s,%s,%s\n' \
    "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$((now - started_at))" \
    "$(systemctl --user show swarm-next-api.service -p MemoryCurrent --value)" \
    "$(systemctl --user show swarm-next-api.service -p TasksCurrent --value)" \
    "$(systemctl --user show swarm-next-terminal-host.service -p MemoryCurrent --value)" \
    "$(systemctl --user show swarm-next-terminal-host.service -p TasksCurrent --value)" \
    "${running_sessions}" "${retained_sessions}" \
    "${history_bytes}" "${dropped_history_bytes}" >>"${samples_file}"
  sample_count=$((sample_count + 1))
  sleep "${sample_seconds}"
done

jq -n \
  --arg run_id "${run_id}" \
  --arg session_a "${session_a}" \
  --arg session_b "${session_b}" \
  --arg samples_file "${samples_file}" \
  --argjson duration_seconds "${duration_seconds}" \
  --argjson sample_count "${sample_count}" \
  '{run_id:$run_id, result:"passed", session_ids:[$session_a,$session_b], duration_seconds:$duration_seconds, sample_count:$sample_count, samples_file:$samples_file}' \
  >"${summary_file}"
cat "${summary_file}"

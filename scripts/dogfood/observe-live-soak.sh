#!/usr/bin/env bash
set -euo pipefail

# Read-only soak for an already-running dogfood Hive. It never starts, stops,
# writes to, or restarts a worker or service. The operator token is kept in a
# mode-0600 curl config and never appears in process arguments or reports.

base_url="${SWARM_SOAK_BASE_URL:-http://127.0.0.1:8766}"
duration_seconds="${SWARM_SOAK_DURATION_SECONDS:-3600}"
sample_seconds="${SWARM_SOAK_SAMPLE_SECONDS:-30}"
report_dir="${SWARM_SOAK_REPORT_DIR:-${HOME}/.local/state/swarm-next/soak}"

if [[ -z "${SWARM_OPERATOR_TOKEN:-}" ]]; then
  echo "SWARM_OPERATOR_TOKEN is required" >&2
  exit 2
fi
for value in "${duration_seconds}" "${sample_seconds}"; do
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
run_id="$(date -u +%Y%m%dT%H%M%SZ)-live"
samples_file="${report_dir}/${run_id}-samples.csv"
summary_file="${report_dir}/${run_id}-summary.json"
curl_config="$(mktemp "${TMPDIR:-/tmp}/swarm-next-live-soak.XXXXXX")"
chmod 600 "${curl_config}"
printf 'header = "Authorization: Bearer %s"\n' "${SWARM_OPERATOR_TOKEN}" >"${curl_config}"
trap 'rm -f "${curl_config}"' EXIT INT TERM

api_json() {
  curl --fail --silent --show-error --config "${curl_config}" --max-time 5 "$@"
}

initial_sessions="$(api_json "${base_url}/api/v1/terminal/sessions")"
mapfile -t session_ids < <(jq -er '.sessions | map(select(.running == true) | .session_id) | sort | .[]' <<<"${initial_sessions}")
if (( ${#session_ids[@]} == 0 )); then
  echo "no running sessions are available to observe" >&2
  exit 1
fi

initial_host="$(api_json "${base_url}/api/v1/runtime/terminal-host")"
host_version="$(jq -er '.status.host_version' <<<"${initial_host}")"
host_pid="$(systemctl --user show swarm-next-terminal-host.service -p MainPID --value)"
initial_health="$(curl --fail --silent --show-error --max-time 5 "${base_url}/health")"
api_version="$(jq -er '.version' <<<"${initial_health}")"
api_pid="$(systemctl --user show swarm-next-api.service -p MainPID --value)"
printf 'timestamp_utc,elapsed_seconds,api_memory_bytes,api_tasks,terminal_host_memory_bytes,terminal_host_tasks,running_sessions,retained_sessions,history_bytes,dropped_history_bytes\n' >"${samples_file}"

started_at="$(date +%s)"
deadline=$((started_at + duration_seconds))
sample_count=0
while (( $(date +%s) < deadline )); do
  now="$(date +%s)"
  curl --fail --silent --show-error --max-time 5 "${base_url}/health" >/dev/null
  sessions="$(api_json "${base_url}/api/v1/terminal/sessions")"
  for session_id in "${session_ids[@]}"; do
    jq -e --arg id "${session_id}" '.sessions[] | select(.session_id == $id and .running == true)' <<<"${sessions}" >/dev/null
  done
  current_host_pid="$(systemctl --user show swarm-next-terminal-host.service -p MainPID --value)"
  if [[ "${current_host_pid}" != "${host_pid}" ]]; then
    echo "terminal host changed from ${host_pid} to ${current_host_pid}" >&2
    exit 1
  fi
  current_api_pid="$(systemctl --user show swarm-next-api.service -p MainPID --value)"
  if [[ "${current_api_pid}" != "${api_pid}" ]]; then
    echo "API changed from ${api_pid} to ${current_api_pid}; its memory series is no longer continuous" >&2
    exit 1
  fi
  host_status="$(api_json "${base_url}/api/v1/runtime/terminal-host")"
  history="$(api_json "${base_url}/api/v1/terminal/history/diagnostics")"
  printf '%s,%s,%s,%s,%s,%s,%s,%s,%s,%s\n' \
    "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$((now - started_at))" \
    "$(systemctl --user show swarm-next-api.service -p MemoryCurrent --value)" \
    "$(systemctl --user show swarm-next-api.service -p TasksCurrent --value)" \
    "$(systemctl --user show swarm-next-terminal-host.service -p MemoryCurrent --value)" \
    "$(systemctl --user show swarm-next-terminal-host.service -p TasksCurrent --value)" \
    "$(jq -er '.status.running_sessions | numbers' <<<"${host_status}")" \
    "$(jq -er '.status.retained_sessions | numbers' <<<"${host_status}")" \
    "$(jq -er '.diagnostics.retained_bytes | numbers' <<<"${history}")" \
    "$(jq -er '.diagnostics.dropped_bytes | numbers' <<<"${history}")" >>"${samples_file}"
  sample_count=$((sample_count + 1))
  sleep "${sample_seconds}"
done

summary_stats="$(
  awk -F, 'NR == 2 { amin=$3; amax=$3; hmin=$5; hmax=$5; rmin=$9; rmax=$9; dmax=$10 }
    NR > 2 { if ($3 < amin) amin=$3; if ($3 > amax) amax=$3; if ($5 < hmin) hmin=$5; if ($5 > hmax) hmax=$5; if ($9 < rmin) rmin=$9; if ($9 > rmax) rmax=$9; if ($10 > dmax) dmax=$10 }
    END { print amin, amax, hmin, hmax, rmin, rmax, dmax }' "${samples_file}"
)"
read -r api_min api_max host_min host_max history_min history_max dropped_max <<<"${summary_stats}"
for value in "${api_min}" "${api_max}" "${host_min}" "${host_max}" "${history_min}" "${history_max}" "${dropped_max}"; do
  if [[ ! "${value}" =~ ^[0-9]+$ ]]; then
    echo "could not summarize content-free soak samples" >&2
    exit 1
  fi
done
jq -n \
  --arg run_id "${run_id}" --arg api_version "${api_version}" --arg api_pid "${api_pid}" \
  --arg host_version "${host_version}" --arg host_pid "${host_pid}" \
  --arg samples_file "${samples_file}" --argjson duration_seconds "${duration_seconds}" \
  --argjson sample_count "${sample_count}" --argjson observed_sessions "${#session_ids[@]}" \
  --argjson api_memory_min "${api_min}" --argjson api_memory_max "${api_max}" \
  --argjson host_memory_min "${host_min}" --argjson host_memory_max "${host_max}" \
  --argjson history_bytes_min "${history_min}" --argjson history_bytes_max "${history_max}" \
  --argjson dropped_history_bytes_max "${dropped_max}" \
  '{run_id:$run_id,result:"passed",mode:"read_only_live",duration_seconds:$duration_seconds,sample_count:$sample_count,observed_sessions:$observed_sessions,api:{version:$api_version,pid:$api_pid},terminal_host:{version:$host_version,pid:$host_pid},memory_bytes:{api:{min:$api_memory_min,max:$api_memory_max},terminal_host_cgroup:{min:$host_memory_min,max:$host_memory_max}},history_bytes:{min:$history_bytes_min,max:$history_bytes_max,dropped_max:$dropped_history_bytes_max},samples_file:$samples_file}' \
  >"${summary_file}"
cat "${summary_file}"

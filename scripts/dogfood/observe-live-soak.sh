#!/usr/bin/env bash
set -euo pipefail

# Read-only soak for an already-running dogfood Hive. It never starts, stops,
# writes to, or restarts a worker or service. The operator token is kept in a
# mode-0600 curl config and never appears in process arguments or reports.

base_url="${SWARM_SOAK_BASE_URL:-http://127.0.0.1:8766}"
duration_seconds="${SWARM_SOAK_DURATION_SECONDS:-3600}"
sample_seconds="${SWARM_SOAK_SAMPLE_SECONDS:-30}"
report_dir="${SWARM_SOAK_REPORT_DIR:-${HOME}/.local/state/swarm-next/soak}"
api_unit="${SWARM_SOAK_API_UNIT:-swarm-api.service}"
host_unit="${SWARM_SOAK_HOST_UNIT:-swarm-terminal-host.service}"

metric() {
  local value
  value=$(systemctl --user show "$1" -p "$2" --value)
  if [[ ! $value =~ ^[0-9]+$ ]]; then
    echo "Unavailable metric $2 for $1; refusing a misleading sample" >&2
    return 1
  fi
  printf '%s' "$value"
}

# /proc stat fields 14+15 are this process's user/system CPU ticks (not
# children's CPU). Strip the parenthesized command before splitting: it may
# contain spaces or parentheses. Field 22 identifies this process lifetime.
process_cpu() {
  local stat tail
  IFS= read -r stat <"/proc/$1/stat"
  tail=${stat##*) }
  awk '{ if (NF < 20 || $12 !~ /^[0-9]+$/ || $13 !~ /^[0-9]+$/ || $20 !~ /^[0-9]+$/) exit 1; printf "%.0f %s\n", $12+$13, $20 }' <<<"$tail"
}

# RSS attribution is separate from cgroup memory, which includes charged cache.
# Missing kernel fields are unavailable evidence, never a zero-memory sample.
process_memory() {
  awk '
    /^(VmRSS|RssAnon|RssFile):/ {
      if ($2 !~ /^[0-9]+$/ || $3 != "kB" || seen[$1]++) exit 1
      bytes[$1]=$2*1024
    }
    END {
      if (seen["VmRSS:"] != 1 || seen["RssAnon:"] != 1 || seen["RssFile:"] != 1) exit 1
      printf "%.0f %.0f %.0f\n", bytes["VmRSS:"], bytes["RssAnon:"], bytes["RssFile:"]
    }' "/proc/$1/status"
}

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
phase=initialization
sample_count=0
finish_observation() {
  local status=$?
  if (( status != 0 )); then
    # Never print BASH_COMMAND, arguments, payloads or the authentication config.
    printf 'Observation incomplete: phase=%s exit=%s completed_samples=%s\n' "$phase" "$status" "$sample_count" >&2
  fi
  rm -f "${curl_config}"
}
trap finish_observation EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

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
host_pid="$(metric "$host_unit" MainPID)"
clock_ticks="$(getconf CLK_TCK)"
[[ "$clock_ticks" =~ ^[1-9][0-9]*$ ]] || { echo 'CPU clock rate unavailable' >&2; exit 1; }
read -r initial_engine_cpu engine_started < <(process_cpu "$host_pid")
initial_health="$(curl --fail --silent --show-error --max-time 5 "${base_url}/health")"
api_version="$(jq -er '.version' <<<"${initial_health}")"
api_pid="$(metric "$api_unit" MainPID)"
if (( host_pid == 0 || api_pid == 0 )); then
  echo 'Both measured services must be running; PID zero is not continuity evidence' >&2
  exit 1
fi
printf 'timestamp_utc,elapsed_seconds,api_memory_bytes,api_tasks,terminal_host_memory_bytes,terminal_host_tasks,running_sessions,retained_sessions,history_bytes,dropped_history_bytes,api_cpu_nanoseconds,terminal_host_cpu_nanoseconds,collection_seconds,engine_process_cpu_ticks,engine_process_start_ticks,clock_ticks_per_second,api_process_rss_bytes,api_process_anon_bytes,api_process_file_bytes\n' >"${samples_file}"

started_at="$(date +%s)"
deadline=$((started_at + duration_seconds))
sample_count=0
while (( $(date +%s) < deadline )); do
  now="$(date +%s)"
  phase=health
  curl --fail --silent --show-error --max-time 5 "${base_url}/health" >/dev/null
  phase=session_continuity
  sessions="$(api_json "${base_url}/api/v1/terminal/sessions")"
  for session_id in "${session_ids[@]}"; do
    if ! jq -e --arg id "${session_id}" '.sessions[] | select(.session_id == $id and .running == true)' <<<"${sessions}" >/dev/null; then
      echo 'An original session is no longer observed running; refusing to join different workloads' >&2
      exit 1
    fi
  done
  phase=service_continuity
  current_host_pid="$(metric "$host_unit" MainPID)"
  if [[ "${current_host_pid}" != "${host_pid}" ]]; then
    echo "terminal host changed from ${host_pid} to ${current_host_pid}" >&2
    exit 1
  fi
  current_api_pid="$(metric "$api_unit" MainPID)"
  if [[ "${current_api_pid}" != "${api_pid}" ]]; then
    echo "API changed from ${api_pid} to ${current_api_pid}; its memory series is no longer continuous" >&2
    exit 1
  fi
  phase=metrics
  host_status="$(api_json "${base_url}/api/v1/runtime/terminal-host")"
  history="$(api_json "${base_url}/api/v1/terminal/history/diagnostics")"
  api_memory=$(metric "$api_unit" MemoryCurrent)
  api_process_memory=$(process_memory "$api_pid")
  read -r api_rss api_anon api_file <<<"$api_process_memory"
  api_tasks=$(metric "$api_unit" TasksCurrent)
  host_memory=$(metric "$host_unit" MemoryCurrent)
  host_tasks=$(metric "$host_unit" TasksCurrent)
  api_cpu=$(metric "$api_unit" CPUUsageNSec)
  host_cpu=$(metric "$host_unit" CPUUsageNSec)
  read -r engine_cpu current_engine_started < <(process_cpu "$host_pid")
  if [[ "$current_engine_started" != "$engine_started" ]]; then
    echo 'Engine process lifetime changed; CPU series is not continuous' >&2
    exit 1
  fi
  running_sessions=$(jq -er '.status.running_sessions | numbers' <<<"$host_status")
  retained_sessions=$(jq -er '.status.retained_sessions | numbers' <<<"$host_status")
  history_bytes=$(jq -er '.diagnostics.retained_bytes | numbers' <<<"$history")
  dropped_bytes=$(jq -er '.diagnostics.dropped_bytes | numbers' <<<"$history")
  sampled_at=$(date +%s)
  printf '%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s\n' \
    "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$((sampled_at - started_at))" \
    "$api_memory" "$api_tasks" "$host_memory" "$host_tasks" \
    "$running_sessions" "$retained_sessions" "$history_bytes" "$dropped_bytes" \
    "$api_cpu" "$host_cpu" "$((sampled_at - now))" "$engine_cpu" "$engine_started" "$clock_ticks" \
    "$api_rss" "$api_anon" "$api_file" >>"${samples_file}"
  sample_count=$((sample_count + 1))
  sleep "${sample_seconds}"
done

phase=summary
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
  '{run_id:$run_id,result:"observed",mode:"read_only_live",performance_acceptance:"not_evaluated",duration_seconds:$duration_seconds,sample_count:$sample_count,observed_sessions:$observed_sessions,api:{version:$api_version,pid:$api_pid},terminal_host:{version:$host_version,pid:$host_pid},memory_bytes:{api:{min:$api_memory_min,max:$api_memory_max},terminal_host_cgroup:{min:$host_memory_min,max:$host_memory_max}},history_bytes:{min:$history_bytes_min,max:$history_bytes_max,dropped_max:$dropped_history_bytes_max},samples_file:$samples_file}' \
  >"${summary_file}"
cat "${summary_file}"
exit 0

//! Reads what each workspace has spent with its provider, and serves it.
//!
//! The provider already writes this down. Every assistant message in
//! `~/.claude/projects/<slug>/*.jsonl` carries a `usage` block with its input,
//! cache-write, cache-read and output counts, a timestamp and a model. Swarm
//! already maps a worker to that directory -- `conversation_freshness` has read
//! these same files since August to decide which conversation is newest. It
//! simply never read the numbers.
//!
//! ⚠️ THREE WAYS A MEASUREMENT LIKE THIS READS AS AUTHORITATIVE AND IS WRONG.
//! Each was hit while building the prototype these numbers come from.
//!
//! 1. DOUBLE COUNTING. 135,779 of 283,132 usage lines in a 30-day window on
//!    this Hive were replays of a message already on disk -- 48%. Summing lines
//!    roughly DOUBLES every figure, and inflates long-running workers most,
//!    which is exactly the population a "who is spending" panel compares.
//!    Deduplication lives in the persistence layer, keyed by message id.
//!
//! 2. IDENTITY COLLISION. A project directory is derived from the workspace
//!    PATH, so two workers in one repository -- Platform and Platform · Codex
//!    share `rcg-platform` on this Hive -- land in the same directory and
//!    cannot be told apart. This reports the WORKSPACE, and names every worker
//!    sharing it, rather than attributing to one a number that belongs to both.
//!
//! 3. COST. 5.1GB across 890 transcripts; a full pass takes minutes. The scan
//!    is incremental, off the request path, and single-flighted.

use std::collections::{BTreeMap, HashMap};
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use axum::Json;
use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use swarm_persistence::CountedMessage;

use crate::{ApiError, AppState, authorize, task_store, task_store_error};

/// How much history the panel keeps. Beyond this the daily rows and the
/// replay-guard ids are both dropped, at the same boundary.
const RETAIN_DAYS: i64 = 60;
/// A pass is not repeated more often than this. The transcripts only grow while
/// workers are running, and a panel that is a few minutes stale is honest as
/// long as it says when it was read -- which it does.
const MIN_SECONDS_BETWEEN_SCANS: i64 = 300;
/// Bounds one pass so a Hive with an enormous history cannot wedge the worker
/// thread. A pass that stops early simply resumes from its cursors next time.
const MAX_BYTES_PER_SCAN: u64 = 3 * 1024 * 1024 * 1024;

#[derive(Debug, Deserialize)]
pub(super) struct UsageQuery {
    days: Option<i64>,
}

#[derive(Debug, Serialize)]
struct WorkspaceUsage {
    workspace: String,
    /// Every worker configured against this workspace. Plural on purpose: see
    /// identity collision above.
    workers: Vec<String>,
    input_tokens: i64,
    cache_write_tokens: i64,
    cache_read_tokens: i64,
    output_tokens: i64,
    messages: i64,
    /// Billing-weighted, in input-token equivalents, so the four kinds can be
    /// compared as one number. Never a price.
    weighted: i64,
    /// The same figure over the preceding window of equal length, so a reader
    /// can see direction without doing the arithmetic.
    weighted_previous: i64,
}

#[derive(Debug, Serialize)]
struct DayUsage {
    day: String,
    input_tokens: i64,
    cache_write_tokens: i64,
    cache_read_tokens: i64,
    output_tokens: i64,
    weighted: i64,
}

#[derive(Debug, Serialize)]
struct UsageReport {
    days: i64,
    /// Unix seconds of the last completed pass, or null if none has finished.
    /// The panel says this out loud rather than implying the numbers are live.
    last_scan_at: Option<i64>,
    scanning: bool,
    by_workspace: Vec<WorkspaceUsage>,
    by_day: Vec<DayUsage>,
    total_weighted: i64,
    total_weighted_previous: i64,
}

// ⚠️ WEIGHTED IN INTEGERS, NOT FLOATS, and not for tidiness. A busy week here is
// billions of tokens, and f64 carries 53 bits of mantissa -- past 2^53 it starts
// rounding the very figures a "who is spending" panel is compared on. Scaled
// i128 throughout, back to i64 once at the end.
//
// Ratios relative to one input token, times SCALE. They hold across Opus,
// Sonnet and Haiku; only the absolute price differs, which `model_milli` carries.
const SCALE: i128 = 100;
const W_INPUT: i128 = 100;
const W_CACHE_WRITE: i128 = 125;
const W_CACHE_READ: i128 = 10;
const W_OUTPUT: i128 = 500;

/// Opus costs about five times Sonnet per token, and Haiku far less, given here
/// in thousandths. Without this a day of Haiku compares to a day of Opus as
/// though they cost the same. An unrecognised model is assumed expensive rather
/// than flattering the total.
fn model_milli(model: &str) -> i128 {
    let model = model.to_ascii_lowercase();
    if model.contains("sonnet") {
        200
    } else if model.contains("haiku") {
        55
    } else {
        1000
    }
}

fn weigh(input: i64, cache_write: i64, cache_read: i64, output: i64, model: &str) -> i64 {
    let raw = i128::from(input) * W_INPUT
        + i128::from(cache_write) * W_CACHE_WRITE
        + i128::from(cache_read) * W_CACHE_READ
        + i128::from(output) * W_OUTPUT;
    i64::try_from(raw * model_milli(model) / 1000 / SCALE).unwrap_or(i64::MAX)
}

/// One assistant message's counts, as they appear in a transcript line.
#[derive(Debug, Deserialize)]
struct TranscriptLine {
    #[serde(rename = "type")]
    kind: Option<String>,
    timestamp: Option<String>,
    message: Option<TranscriptMessage>,
}

#[derive(Debug, Deserialize)]
struct TranscriptMessage {
    id: Option<String>,
    model: Option<String>,
    usage: Option<TranscriptUsage>,
}

/// The provider's own field names, carried by serde. The Rust names drop the
/// `_tokens` postfix every one of them would otherwise share.
#[derive(Debug, Default, Deserialize)]
struct TranscriptUsage {
    #[serde(default, rename = "input_tokens")]
    input: i64,
    #[serde(default, rename = "cache_creation_input_tokens")]
    cache_write: i64,
    #[serde(default, rename = "cache_read_input_tokens")]
    cache_read: i64,
    #[serde(default, rename = "output_tokens")]
    output: i64,
}

/// Pulls the counted messages out of one transcript, starting at `from_offset`.
///
/// Returns the messages and the offset reached. Reading resumes from a byte
/// offset rather than re-reading the file, which is what keeps a 5.1GB corpus
/// affordable after the first pass.
///
/// ⚠️ A FILE SHORTER THAN ITS CURSOR WAS REWRITTEN, NOT APPENDED TO, and is
/// read from zero. Trusting the cursor there would skip the whole file forever
/// while reporting success -- the silent-negative shape, where a check that
/// measures nothing is indistinguishable from a check that found nothing.
fn read_transcript(
    path: &Path,
    workspace: &str,
    from_offset: u64,
) -> std::io::Result<(Vec<CountedMessage>, u64)> {
    let file = std::fs::File::open(path)?;
    let length = file.metadata()?.len();
    let start = if from_offset > length { 0 } else { from_offset };
    let mut reader = BufReader::new(file);
    reader.seek(SeekFrom::Start(start))?;

    let mut messages = Vec::new();
    let mut consumed = start;
    let mut line = String::new();
    loop {
        line.clear();
        let read = reader.read_line(&mut line)?;
        if read == 0 {
            break;
        }
        // ⚠️ ONLY COUNT A LINE THAT ENDED. A pass can land mid-write while a
        // worker is running; a truncated final line is not a short message, it
        // is half of one. Leaving the cursor before it means the next pass
        // reads it whole.
        if !line.ends_with('\n') {
            break;
        }
        consumed += read as u64;
        if !line.contains("\"usage\"") {
            continue;
        }
        let Ok(parsed) = serde_json::from_str::<TranscriptLine>(&line) else {
            continue;
        };
        if parsed.kind.as_deref() != Some("assistant") {
            continue;
        }
        let (Some(message), Some(timestamp)) = (parsed.message, parsed.timestamp) else {
            continue;
        };
        let (Some(id), Some(usage)) = (message.id, message.usage) else {
            continue;
        };
        // "2026-09-14T12:31:02.123Z" -> "2026-09-14". Taking the date off the
        // front rather than parsing the whole stamp: the day is all that is
        // stored, and a transcript whose timestamp is malformed should be
        // skipped rather than dated to the epoch.
        let Some(day) = timestamp.get(..10) else {
            continue;
        };
        // `!day.as_bytes()[4] == b'-'` parses as bitwise-NOT on the byte and
        // is always false -- the guard would never fire. Written out properly.
        if day.len() != 10 || day.as_bytes()[4] != b'-' || day.as_bytes()[7] != b'-' {
            continue;
        }
        messages.push(CountedMessage {
            message_id: id,
            workspace: workspace.to_owned(),
            day: day.to_owned(),
            model: message.model.unwrap_or_else(|| "unknown".to_owned()),
            input_tokens: usage.input,
            cache_write_tokens: usage.cache_write,
            cache_read_tokens: usage.cache_read,
            output_tokens: usage.output,
        });
    }
    Ok((messages, consumed))
}

/// Walks every configured workspace's transcripts once, folding what is new.
///
/// Returns whether the pass REACHED THE END of the work rather than running out
/// of budget, and the caller must not record a completion unless it did. A
/// first pass on this Hive has 5.1GB of history to get through and stops
/// part-way by design; stamping that as "measured" would put a timestamp on
/// numbers that are still half-read, and the panel prints that timestamp.
fn scan_once(
    store: &swarm_persistence::TaskStore,
    projects_root: &Path,
    workspaces: &[String],
    now: i64,
) -> Result<bool, swarm_persistence::TaskStoreError> {
    let mut budget = MAX_BYTES_PER_SCAN;
    for workspace in workspaces {
        let Some(directory) = swarm_persistence::claude_project_directory(projects_root, workspace)
        else {
            continue;
        };
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            let Some(key) = path.to_str() else { continue };
            let read_from = store
                .provider_usage_cursor(key)?
                .map_or(0, |mark| u64::try_from(mark.offset).unwrap_or(0));
            let size = entry.metadata().map_or(0, |meta| meta.len());
            // Nothing appended since the last pass. The overwhelming majority
            // of files on any given pass.
            if size == read_from {
                continue;
            }
            let to_read = size.saturating_sub(read_from.min(size));
            if to_read > budget {
                return Ok(false);
            }
            budget -= to_read;
            let Ok((messages, reached)) = read_transcript(&path, workspace, read_from) else {
                continue;
            };
            let reached = i64::try_from(reached).unwrap_or(i64::MAX);
            store.record_provider_usage(key, reached, now, &messages)?;
        }
    }
    Ok(true)
}

/// Seconds since the epoch. A clock before 1970 is not a case worth carrying
/// through every caller, so it reads as zero and the panel shows no history.
fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| {
            i64::try_from(since.as_secs()).unwrap_or(i64::MAX)
        })
}

fn day_string(unix: i64) -> String {
    // Days are UTC, matching the transcript timestamps they are cut from. A
    // local-time boundary would move spend between days depending on where the
    // reader is standing, which is worse than a boundary that is merely not
    // midnight where they live.
    let days = unix.div_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}")
}

/// Howard Hinnant's days-from-civil, inverted. Chosen over a date crate because
/// this is the only date arithmetic in the workspace and it is exact.
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

/// Folds the stored rows into the two views the panel draws, and the two
/// totals it compares. Split out of `usage_report` because that handler was
/// over this workspace's hundred-line ceiling, and because the arithmetic is
/// worth reading on its own: a row lands in the current window or the previous
/// one, never both, and only the current window feeds the daily series.
fn aggregate(
    rows: Vec<swarm_persistence::ProviderUsageDay>,
    workers_by_workspace: &BTreeMap<String, Vec<String>>,
    window_start: &str,
) -> (Vec<WorkspaceUsage>, Vec<DayUsage>, i64, i64) {
    let mut by_workspace: HashMap<String, WorkspaceUsage> = HashMap::new();
    let mut by_day: BTreeMap<String, DayUsage> = BTreeMap::new();
    let mut total = 0i64;
    let mut total_previous = 0i64;
    for row in rows {
        let weighted = weigh(
            row.input_tokens,
            row.cache_write_tokens,
            row.cache_read_tokens,
            row.output_tokens,
            &row.model,
        );
        let current = row.day.as_str() >= window_start;
        let entry = by_workspace
            .entry(row.workspace.clone())
            .or_insert_with(|| WorkspaceUsage {
                workspace: row.workspace.clone(),
                workers: workers_by_workspace
                    .get(&row.workspace)
                    .cloned()
                    .unwrap_or_default(),
                input_tokens: 0,
                cache_write_tokens: 0,
                cache_read_tokens: 0,
                output_tokens: 0,
                messages: 0,
                weighted: 0,
                weighted_previous: 0,
            });
        if current {
            entry.input_tokens += row.input_tokens;
            entry.cache_write_tokens += row.cache_write_tokens;
            entry.cache_read_tokens += row.cache_read_tokens;
            entry.output_tokens += row.output_tokens;
            entry.messages += row.messages;
            entry.weighted += weighted;
            total += weighted;
            let day = by_day.entry(row.day.clone()).or_insert_with(|| DayUsage {
                day: row.day.clone(),
                input_tokens: 0,
                cache_write_tokens: 0,
                cache_read_tokens: 0,
                output_tokens: 0,
                weighted: 0,
            });
            day.input_tokens += row.input_tokens;
            day.cache_write_tokens += row.cache_write_tokens;
            day.cache_read_tokens += row.cache_read_tokens;
            day.output_tokens += row.output_tokens;
            day.weighted += weighted;
        } else {
            entry.weighted_previous += weighted;
            total_previous += weighted;
        }
    }

    let mut workspaces: Vec<WorkspaceUsage> = by_workspace
        .into_values()
        .filter(|entry| entry.weighted > 0 || entry.weighted_previous > 0)
        .collect();
    workspaces.sort_by(|a, b| {
        b.weighted
            .cmp(&a.weighted)
            .then_with(|| a.workspace.cmp(&b.workspace))
    });
    (
        workspaces,
        by_day.into_values().collect(),
        total,
        total_previous,
    )
}

/// Serves the stored totals, and kicks off a pass when they are stale.
///
/// The response never waits for a scan. A first-ever pass on this Hive reads
/// 5.1GB and takes minutes; a panel that blocked on it would look broken. So
/// the numbers come back immediately with `last_scan_at` and `scanning` saying
/// exactly how fresh they are, and the panel prints that rather than implying
/// they are live.
pub(super) async fn usage_report(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<UsageQuery>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let days = query.days.unwrap_or(7).clamp(1, RETAIN_DAYS);
    let store = task_store(&state)?;
    let now = unix_now();

    let profiles = store
        .list_worker_profiles()
        .map_err(|error| task_store_error(&error))?;
    // Workspace -> the workers sharing it. Plural because they genuinely can
    // be, and a single name here would be a claim the data cannot support.
    let mut workers_by_workspace: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for profile in &profiles {
        workers_by_workspace
            .entry(profile.workspace.clone())
            .or_default()
            .push(profile.name.clone());
    }

    let last_scan = store
        .provider_usage_last_scan()
        .map_err(|error| task_store_error(&error))?;
    let needs_refresh = last_scan.is_none_or(|at| now - at >= MIN_SECONDS_BETWEEN_SCANS);
    let workspaces: Vec<String> = workers_by_workspace.keys().cloned().collect();
    let scanning = needs_refresh && start_scan(&state, store, workspaces, now);

    // Two windows of equal length, so "up 13%" is arithmetic rather than feel.
    let window_start = day_string(now - (days - 1) * 86_400);
    let previous_start = day_string(now - (2 * days - 1) * 86_400);
    let rows = store
        .provider_usage_since(&previous_start)
        .map_err(|error| task_store_error(&error))?;

    let (workspaces, by_day, total, total_previous) =
        aggregate(rows, &workers_by_workspace, &window_start);

    Ok(Json(UsageReport {
        days,
        last_scan_at: last_scan,
        scanning,
        by_workspace: workspaces,
        by_day,
        total_weighted: total,
        total_weighted_previous: total_previous,
    })
    .into_response())
}

/// Starts a pass unless one is already running. Returns whether one is now in
/// flight, which is what the panel prints.
///
/// Single-flighted with a plain flag rather than a lock held across the work:
/// a second request arriving mid-pass should be told "a scan is running" and
/// given the stored numbers, not made to wait for minutes of file reading.
fn start_scan(
    state: &Arc<AppState>,
    store: &swarm_persistence::TaskStore,
    workspaces: Vec<String>,
    now: i64,
) -> bool {
    if state
        .provider_usage_scanning
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return true;
    }
    let Some(projects_root) = claude_projects_root() else {
        state.provider_usage_scanning.store(false, Ordering::SeqCst);
        return false;
    };
    let store = store.clone();
    let flag: Arc<AtomicBool> = Arc::clone(&state.provider_usage_scanning);
    tokio::task::spawn_blocking(move || {
        // ⚠️ ONLY A PASS THAT FINISHED IS A MEASUREMENT. An incomplete pass
        // leaves `last_completed_at` alone, so the panel keeps saying how old
        // the last REAL figure is, and the next request starts another pass
        // immediately instead of waiting out the five-minute floor on numbers
        // that are still being read.
        if matches!(
            scan_once(&store, &projects_root, &workspaces, now),
            Ok(true)
        ) {
            let keep_from = day_string(now - RETAIN_DAYS * 86_400);
            let _ = store.finish_provider_usage_scan(now, &keep_from);
        }
        flag.store(false, Ordering::SeqCst);
    });
    true
}

fn claude_projects_root() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    Some(
        std::env::var_os("CLAUDE_CONFIG_DIR")
            .map_or_else(|| home.join(".claude"), PathBuf::from)
            .join("projects"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn line(id: &str, day: &str, read: i64) -> String {
        format!(
            r#"{{"type":"assistant","timestamp":"{day}T10:00:00.000Z","sessionId":"s","message":{{"id":"{id}","model":"claude-opus-5","usage":{{"input_tokens":1,"cache_creation_input_tokens":2,"cache_read_input_tokens":{read},"output_tokens":4}}}}}}"#
        )
    }

    #[test]
    fn reads_only_what_was_appended_since_the_cursor() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("t.jsonl");
        let mut file = std::fs::File::create(&path).expect("create");
        writeln!(file, "{}", line("m1", "2026-09-14", 10)).expect("write");
        file.flush().expect("flush");

        let (first, offset) = read_transcript(&path, "/w", 0).expect("first read");
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].cache_read_tokens, 10);

        writeln!(file, "{}", line("m2", "2026-09-14", 20)).expect("append");
        file.flush().expect("flush");

        let (second, _) = read_transcript(&path, "/w", offset).expect("second read");
        assert_eq!(second.len(), 1, "only the appended message is read again");
        assert_eq!(second[0].message_id, "m2");
    }

    /// ⚠️ A shorter file was REWRITTEN, so the cursor is meaningless and the
    /// whole file is read again. Trusting it would skip the file forever while
    /// reporting success.
    #[test]
    fn a_truncated_transcript_is_read_from_the_beginning() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("t.jsonl");
        std::fs::write(&path, format!("{}\n", line("m1", "2026-09-14", 10))).expect("write");

        let (messages, _) = read_transcript(&path, "/w", 10_000).expect("read");

        assert_eq!(messages.len(), 1, "the stale cursor did not hide the file");
    }

    /// A pass can land mid-write. Half a line is not a short message.
    #[test]
    fn a_half_written_final_line_waits_for_the_next_pass() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("t.jsonl");
        let complete = format!("{}\n", line("m1", "2026-09-14", 10));
        std::fs::write(&path, format!("{complete}{{\"type\":\"assist")).expect("write");

        let (messages, offset) = read_transcript(&path, "/w", 0).expect("read");

        assert_eq!(messages.len(), 1);
        assert_eq!(
            offset,
            complete.len() as u64,
            "the cursor stops before the partial line so the next pass reads it whole",
        );
    }

    #[test]
    fn non_assistant_and_unparseable_lines_are_skipped_rather_than_counted() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("t.jsonl");
        std::fs::write(
            &path,
            format!(
                "{{\"type\":\"user\",\"usage\":\"not a block\"}}\nnot json at all\n{}\n",
                line("m1", "2026-09-14", 10)
            ),
        )
        .expect("write");

        let (messages, _) = read_transcript(&path, "/w", 0).expect("read");

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].message_id, "m1");
    }

    /// Without the model weight a day of Haiku compares to a day of Opus as
    /// though they cost the same.
    #[test]
    fn weighting_separates_the_models_and_the_token_kinds() {
        // Cache reads are a tenth of input, output is five times it.
        assert_eq!(weigh(0, 0, 1000, 0, "claude-opus-5"), 100);
        assert_eq!(weigh(0, 0, 0, 1000, "claude-opus-5"), 5000);
        assert_eq!(weigh(0, 1000, 0, 0, "claude-opus-5"), 1250);
        // Same tokens, cheaper model.
        assert_eq!(weigh(1000, 0, 0, 0, "claude-sonnet-5"), 200);
        assert_eq!(weigh(1000, 0, 0, 0, "claude-haiku-4-5"), 55);
        // An unknown model is assumed expensive rather than flattering the bill.
        assert_eq!(weigh(1000, 0, 0, 0, "something-new"), 1000);
    }

    #[test]
    fn days_are_utc_and_match_the_transcript_timestamps() {
        // 2026-09-14T12:31:02Z
        assert_eq!(day_string(1_789_389_062), "2026-09-14");
        // Boundaries, where an off-by-one would move a whole day of spend.
        assert_eq!(day_string(1_789_344_000), "2026-09-14");
        assert_eq!(day_string(1_789_343_999), "2026-09-13");
    }
}

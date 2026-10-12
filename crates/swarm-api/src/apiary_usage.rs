//! Usage across an Apiary (ADR 0114 addendum): each Hive's worker-busy time,
//! what each member reports it spent, and the Keeper's totals.

use std::{
    collections::{BTreeMap, HashSet},
    sync::{Arc, Mutex},
};

use axum::{
    Json,
    extract::{Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use swarm_application::ApiaryService;
use swarm_domain::{
    HIVE_USAGE_SCHEMA_VERSION, HiveId, HiveUsageReport, MAX_USAGE_LABEL_BYTES, MAX_USAGE_ROWS,
    USAGE_REPORT_DAYS, UsageActive, UsageTokens, WorkerProfile, WorkerSessionId,
    clip_diagnostic_text,
};
use swarm_persistence::TaskStore;

use crate::{
    ApiError, AppState, apiary_service, application_error, authorize, federation_http,
    federation_node_credential,
    provider_usage::{RETAIN_DAYS, day_string, weigh},
    unix_timestamp,
};

/// The longest gap between two looks at the workers that is still counted as
/// busy. Passes come every half minute or sooner; a longer gap is a machine
/// that slept or a process that stopped, and none of it was work.
const MAX_BUSY_INTERVAL_SECONDS: i64 = 90;
/// The least time between two looks at this Hive's usage for its Keeper.
const USAGE_SAMPLE_SECONDS: i64 = 60;
/// How often an unchanged report is sent again.
const USAGE_RESEND_SECONDS: i64 = 600;

/// Adds the time since the last look to each workspace whose worker was busy
/// at that look, on this Hive.
///
/// ⚠️ MEASURED FROM THE LOOKS THAT ALREADY HAPPEN, NOT A TIMER OF ITS OWN. Each
/// interval is credited to the state seen at its start, and an interval longer
/// than a pass ever takes is cut short: an approximation of busy time, never a
/// stopwatch, and it says so where it is shown.
pub(crate) fn record_busy_time(
    state: &AppState,
    profiles: &[WorkerProfile],
    busy: &HashSet<WorkerSessionId>,
    now: i64,
) {
    let Some(store) = state.task_store.as_ref() else {
        return;
    };
    let previous = state
        .busy_sampled_at
        .lock()
        .ok()
        .and_then(|mut at| at.replace(now));
    let Some(previous) = previous else {
        return;
    };
    let elapsed = now
        .saturating_sub(previous)
        .clamp(0, MAX_BUSY_INTERVAL_SECONDS);
    if elapsed == 0 || busy.is_empty() {
        return;
    }
    let mut by_workspace = BTreeMap::<String, i64>::new();
    for profile in profiles {
        if profile
            .active_session_id
            .is_some_and(|session| busy.contains(&session))
        {
            *by_workspace.entry(profile.workspace.clone()).or_default() += elapsed;
        }
    }
    let rows = by_workspace.into_iter().collect::<Vec<_>>();
    if let Err(error) = store.record_worker_active_seconds(
        &day_string(now),
        &rows,
        &day_string(now - RETAIN_DAYS * 86_400),
    ) {
        tracing::debug!(%error, "workers' busy time could not be recorded");
    }
}

/// What this member last sent its Keeper about its usage.
#[derive(Default)]
pub(crate) struct UsageShipping {
    sampled_at: Option<i64>,
    sent: Option<(HiveUsageReport, i64)>,
}

pub(crate) type SharedUsageShipping = Arc<Mutex<UsageShipping>>;

/// Sends this member's usage to its Keeper when it has changed, or every ten
/// minutes when it has not. Paced like the scan it reads, which runs at most
/// every five minutes.
pub(crate) async fn ship_usage(state: &AppState) {
    let Some(store) = state.task_store.as_ref() else {
        return;
    };
    let Ok(connection) = ApiaryService::new(store.clone()).federation_member_connection() else {
        return;
    };
    let now = unix_timestamp();
    {
        let Ok(mut shipping) = state.usage_shipping.lock() else {
            return;
        };
        if shipping
            .sampled_at
            .is_some_and(|at| now.saturating_sub(at) < USAGE_SAMPLE_SECONDS)
        {
            return;
        }
        shipping.sampled_at = Some(now);
    }
    let report = derive_usage_report(store, now);
    let due = state.usage_shipping.lock().is_ok_and(|shipping| {
        shipping.sent.as_ref().is_none_or(|(sent, at)| {
            sent.tokens != report.tokens
                || sent.active != report.active
                || now.saturating_sub(*at) >= USAGE_RESEND_SECONDS
        })
    });
    if !due {
        return;
    }
    let Ok(client) = federation_http::FederationHttpClient::new(&connection.keeper_endpoint) else {
        return;
    };
    match client
        .publish_usage(&connection.node_credential, &report)
        .await
    {
        // An older Keeper has no such route: nothing to retry before the next
        // resend.
        Ok(()) | Err(federation_http::FederationHttpError::RemoteRejected(404 | 405)) => {
            if let Ok(mut shipping) = state.usage_shipping.lock() {
                shipping.sent = Some((report, now));
            }
        }
        Err(error) => {
            tracing::debug!(%error, "this Hive's usage could not reach its Keeper; it tries again next pass");
        }
    }
}

/// This Hive's tokens and busy time over the report's window, within the
/// report's bounds: the newest rows are kept if there are too many.
fn derive_usage_report(store: &TaskStore, now: i64) -> HiveUsageReport {
    let from = day_string(now - (USAGE_REPORT_DAYS - 1) * 86_400);
    let label = |text: &str| {
        if text.is_empty() {
            "unknown".to_owned()
        } else {
            clip_diagnostic_text(text, MAX_USAGE_LABEL_BYTES)
        }
    };
    let newest = |count: usize| count.saturating_sub(MAX_USAGE_ROWS);
    let tokens = store.provider_usage_since(&from).unwrap_or_default();
    let tokens = tokens[newest(tokens.len())..]
        .iter()
        .map(|row| UsageTokens {
            day: row.day.clone(),
            workspace: label(&row.workspace),
            model: label(&row.model),
            input_tokens: row.input_tokens.max(0),
            cache_write_tokens: row.cache_write_tokens.max(0),
            cache_read_tokens: row.cache_read_tokens.max(0),
            output_tokens: row.output_tokens.max(0),
            messages: row.messages.max(0),
        })
        .collect();
    let active = store.worker_active_since(&from).unwrap_or_default();
    let active = active[newest(active.len())..]
        .iter()
        .map(|row| UsageActive {
            workspace: label(&row.workspace),
            ..row.clone()
        })
        .collect();
    HiveUsageReport {
        schema_version: HIVE_USAGE_SCHEMA_VERSION,
        observed_at: now,
        tokens,
        active,
    }
}

/// A member's usage, received by its Keeper.
pub(crate) async fn accept_member_usage(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(report): Json<HiveUsageReport>,
) -> Result<Response, ApiError> {
    let credential = federation_node_credential(&headers)?;
    report
        .validate()
        .map_err(|reason| ApiError::new(StatusCode::BAD_REQUEST, "invalid_usage_report", reason))?;
    let store = crate::task_store(&state)?;
    store
        .accept_hive_usage(credential, &report, unix_timestamp())
        .map_err(|_| {
            ApiError::new(
                StatusCode::UNAUTHORIZED,
                "invalid_federation_credential",
                "a current federation node credential is required",
            )
        })?;
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        StatusCode::NO_CONTENT,
    )
        .into_response())
}

#[derive(Debug, Deserialize)]
pub(crate) struct ApiaryUsageQuery {
    days: Option<i64>,
}

/// Spend in one bucket. `weighted` puts the four kinds of token on one scale,
/// in input-token equivalents and by model; it is never a price.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
struct Totals {
    input_tokens: i64,
    cache_write_tokens: i64,
    cache_read_tokens: i64,
    output_tokens: i64,
    messages: i64,
    weighted: i64,
    active_seconds: i64,
}

impl Totals {
    fn add_tokens(&mut self, row: &UsageTokens) {
        self.input_tokens = self.input_tokens.saturating_add(row.input_tokens);
        self.cache_write_tokens = self
            .cache_write_tokens
            .saturating_add(row.cache_write_tokens);
        self.cache_read_tokens = self.cache_read_tokens.saturating_add(row.cache_read_tokens);
        self.output_tokens = self.output_tokens.saturating_add(row.output_tokens);
        self.messages = self.messages.saturating_add(row.messages);
        self.weighted = self.weighted.saturating_add(weigh(
            row.input_tokens,
            row.cache_write_tokens,
            row.cache_read_tokens,
            row.output_tokens,
            &row.model,
        ));
    }

    fn add(&mut self, other: &Self) {
        self.input_tokens = self.input_tokens.saturating_add(other.input_tokens);
        self.cache_write_tokens = self
            .cache_write_tokens
            .saturating_add(other.cache_write_tokens);
        self.cache_read_tokens = self
            .cache_read_tokens
            .saturating_add(other.cache_read_tokens);
        self.output_tokens = self.output_tokens.saturating_add(other.output_tokens);
        self.messages = self.messages.saturating_add(other.messages);
        self.weighted = self.weighted.saturating_add(other.weighted);
        self.active_seconds = self.active_seconds.saturating_add(other.active_seconds);
    }
}

#[derive(Debug, Serialize)]
struct Named {
    name: String,
    #[serde(flatten)]
    totals: Totals,
}

#[derive(Debug, Serialize)]
struct HiveUsageView {
    hive_id: HiveId,
    is_local: bool,
    /// When the Keeper last heard this Hive's figures; null if it never has.
    /// The Keeper's own are read in place, so they are as fresh as its scan.
    received_at: Option<i64>,
    totals: Totals,
    by_model: Vec<Named>,
    by_workspace: Vec<Named>,
}

#[derive(Debug, Serialize)]
struct ApiaryUsageView {
    days: i64,
    hives: Vec<HiveUsageView>,
    totals: Totals,
    by_model: Vec<Named>,
}

/// One Hive's figures within the window, by model and by workspace.
fn summarise(
    hive_id: HiveId,
    is_local: bool,
    received_at: Option<i64>,
    tokens: &[UsageTokens],
    active: &[UsageActive],
    window_start: &str,
) -> HiveUsageView {
    let mut totals = Totals::default();
    let mut by_model = BTreeMap::<String, Totals>::new();
    let mut by_workspace = BTreeMap::<String, Totals>::new();
    for row in tokens.iter().filter(|row| row.day.as_str() >= window_start) {
        totals.add_tokens(row);
        by_model
            .entry(row.model.clone())
            .or_default()
            .add_tokens(row);
        by_workspace
            .entry(row.workspace.clone())
            .or_default()
            .add_tokens(row);
    }
    for row in active.iter().filter(|row| row.day.as_str() >= window_start) {
        totals.active_seconds = totals.active_seconds.saturating_add(row.seconds);
        let workspace = by_workspace.entry(row.workspace.clone()).or_default();
        workspace.active_seconds = workspace.active_seconds.saturating_add(row.seconds);
    }
    HiveUsageView {
        hive_id,
        is_local,
        received_at,
        totals,
        by_model: ranked(by_model),
        by_workspace: ranked(by_workspace),
    }
}

/// Biggest spender first, then busiest.
fn ranked(buckets: BTreeMap<String, Totals>) -> Vec<Named> {
    let mut named = buckets
        .into_iter()
        .map(|(name, totals)| Named { name, totals })
        .collect::<Vec<_>>();
    named.sort_by(|left, right| {
        right
            .totals
            .weighted
            .cmp(&left.totals.weighted)
            .then(right.totals.active_seconds.cmp(&left.totals.active_seconds))
    });
    named
}

/// What every Hive in this Keeper's Apiary has spent, and the Apiary's totals.
pub(crate) async fn apiary_usage(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<ApiaryUsageQuery>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let days = query.days.unwrap_or(7).clamp(1, USAGE_REPORT_DAYS);
    let now = unix_timestamp();
    let window_start = day_string(now - (days - 1) * 86_400);
    let store = crate::task_store(&state)?;
    let service = apiary_service(&state)?;
    let reports = store
        .apiary_hive_usage()
        .map_err(|error| crate::task_store_error(&error))?;
    let mut hives = Vec::new();
    for member in service.members().map_err(application_error)? {
        if member.is_local {
            let tokens = store
                .provider_usage_since(&window_start)
                .map_err(|error| crate::task_store_error(&error))?
                .into_iter()
                .map(|row| UsageTokens {
                    day: row.day,
                    workspace: row.workspace,
                    model: row.model,
                    input_tokens: row.input_tokens,
                    cache_write_tokens: row.cache_write_tokens,
                    cache_read_tokens: row.cache_read_tokens,
                    output_tokens: row.output_tokens,
                    messages: row.messages,
                })
                .collect::<Vec<_>>();
            let active = store
                .worker_active_since(&window_start)
                .map_err(|error| crate::task_store_error(&error))?;
            hives.push(summarise(
                member.hive_id,
                true,
                None,
                &tokens,
                &active,
                &window_start,
            ));
            continue;
        }
        let report = reports
            .iter()
            .find(|stored| stored.hive_id == member.hive_id);
        hives.push(summarise(
            member.hive_id,
            false,
            report.map(|stored| stored.received_at),
            report.map_or(&[][..], |stored| &stored.report.tokens),
            report.map_or(&[][..], |stored| &stored.report.active),
            &window_start,
        ));
    }
    let mut totals = Totals::default();
    let mut by_model = BTreeMap::<String, Totals>::new();
    for hive in &hives {
        totals.add(&hive.totals);
        for model in &hive.by_model {
            by_model
                .entry(model.name.clone())
                .or_default()
                .add(&model.totals);
        }
    }
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(ApiaryUsageView {
            days,
            hives,
            totals,
            by_model: ranked(by_model),
        }),
    )
        .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ⚠️ AN APPROXIMATION THAT KNOWS IT IS ONE. Time goes to whoever was busy
    /// at the start of each interval, and a gap longer than a pass ever takes —
    /// a machine asleep — counts for no more than one pass.
    #[test]
    fn busy_time_is_credited_from_the_looks_that_happen_and_a_long_gap_is_cut_short() {
        let store = TaskStore::in_memory().unwrap();
        let state = AppState::default().with_task_store(store.clone());
        let mut queen = store.ensure_queen("/w/queen").unwrap();
        let session = WorkerSessionId::new();
        queen.active_session_id = Some(session);
        let profiles = [queen];
        let busy = HashSet::from([session]);
        let idle = HashSet::new();
        let now = 1_791_000_000;

        // The first look only starts the clock.
        record_busy_time(&state, &profiles, &busy, now);
        record_busy_time(&state, &profiles, &busy, now + 30);
        // Idle at the look before: nothing for that interval.
        record_busy_time(&state, &profiles, &idle, now + 60);
        // Busy, then the machine sleeps for an hour.
        record_busy_time(&state, &profiles, &busy, now + 3_660);

        let recorded = store.worker_active_since(&day_string(now)).unwrap();
        assert_eq!(
            recorded
                .iter()
                .map(|row| (row.workspace.as_str(), row.seconds))
                .collect::<Vec<_>>(),
            [("/w/queen", 30 + MAX_BUSY_INTERVAL_SECONDS)]
        );
    }
}

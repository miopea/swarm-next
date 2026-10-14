//! Fleet health (ADR 0114): a member telling its Keeper how it is, and the
//! Keeper's view of every member — one status each, and a day of contact.

use std::sync::{Arc, Mutex};

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Serialize;
use swarm_application::ApiaryService;
use swarm_domain::{
    CONTACT_HISTORY_SECONDS, CONTACT_SLOT_SECONDS, EngineAttemptOutcome,
    HIVE_HEALTH_SCHEMA_VERSION, HiveCondition, HiveHealthReport, HiveId, ReportedDisk,
    ReportedEngine, ReportedSync, contact_slot,
};
use swarm_persistence::{TaskStore, WorkerEngineUpdateOutcome};
use swarm_terminal::{HostRequest, HostResponse};

use crate::{
    ApiError, AppState, apiary_service, application_error, authorize, build_version,
    federation_http, federation_node_credential, unix_timestamp,
};

/// The least time between two looks at this Hive's own health. Passes can run
/// several times a second while the Keeper is busy; sampling disk and the
/// engine that often would cost more than it tells anyone.
const HEALTH_SAMPLE_SECONDS: i64 = 10;
/// How often an unchanged report is sent again, so the Keeper knows the one it
/// holds is still true.
const HEALTH_RESEND_SECONDS: i64 = 60;

/// What this member last sent its Keeper, so an unchanged report waits.
#[derive(Default)]
pub(crate) struct HealthShipping {
    sampled_at: Option<i64>,
    sent: Option<(HiveHealthReport, i64)>,
}

pub(crate) type SharedHealthShipping = Arc<Mutex<HealthShipping>>;

/// Sends this member's health to its Keeper when it has changed, or once a
/// minute when it has not.
///
/// ⚠️ BEFORE THE PACING AND REFUSAL GATES, as the log is (ADR 0112). A member
/// stopped at a sync step is exactly the one whose Keeper needs to hear which
/// step, and those gates return before anything else is sent.
pub(crate) async fn ship_health(state: &AppState) {
    let Some(store) = state.task_store.as_ref() else {
        return;
    };
    let Ok(connection) = ApiaryService::new(store.clone()).federation_member_connection() else {
        return;
    };
    let now = unix_timestamp();
    {
        let Ok(mut shipping) = state.health_shipping.lock() else {
            return;
        };
        if shipping
            .sampled_at
            .is_some_and(|at| now.saturating_sub(at) < HEALTH_SAMPLE_SECONDS)
        {
            return;
        }
        shipping.sampled_at = Some(now);
    }
    let report = derive_health_report(state, store, now).await;
    let due = state.health_shipping.lock().is_ok_and(|shipping| {
        shipping.sent.as_ref().is_none_or(|(sent, at)| {
            !same_health(sent, &report) || now.saturating_sub(*at) >= HEALTH_RESEND_SECONDS
        })
    });
    if !due {
        return;
    }
    let Ok(client) = federation_http::FederationHttpClient::new(&connection.keeper_endpoint) else {
        return;
    };
    match client
        .publish_health(&connection.node_credential, &report)
        .await
    {
        // A Keeper on an older release has no such route: an ordinary
        // rolling-update state, and nothing to retry before the next minute.
        Ok(()) | Err(federation_http::FederationHttpError::RemoteRejected(404 | 405)) => {
            if let Ok(mut shipping) = state.health_shipping.lock() {
                shipping.sent = Some((report, now));
            }
        }
        Err(error) => {
            tracing::debug!(%error, "this Hive's health could not reach its Keeper; it tries again next pass");
        }
    }
}

/// Two reports say the same thing, whenever each was put together.
fn same_health(left: &HiveHealthReport, right: &HiveHealthReport) -> bool {
    HiveHealthReport {
        observed_at: right.observed_at,
        ..left.clone()
    } == *right
}

/// This Hive's own account of itself. Everything it cannot read is reported as
/// the absence of a problem only where absence is the honest answer: an engine
/// that cannot be asked is not "pending", and storage that cannot be read is
/// `unavailable`, never `normal`.
async fn derive_health_report(state: &AppState, store: &TaskStore, now: i64) -> HiveHealthReport {
    let sync = store.federation_sync_health().unwrap_or_default();
    let update_pending = match &state.terminal_host {
        Some(client) => matches!(
            client.request(&HostRequest::HostStatus).await,
            Ok(HostResponse::HostStatus { status })
                if crate::maintenance::worker_engine_update_required(&status)
        ),
        None => false,
    };
    let last_attempt =
        store
            .last_worker_engine_update()
            .ok()
            .flatten()
            .map(|attempt| match attempt.outcome {
                Some(WorkerEngineUpdateOutcome::Succeeded) => EngineAttemptOutcome::Succeeded,
                Some(WorkerEngineUpdateOutcome::Failed) => EngineAttemptOutcome::Failed,
                Some(WorkerEngineUpdateOutcome::TimedOut) => EngineAttemptOutcome::TimedOut,
                None => EngineAttemptOutcome::Unfinished,
            });
    let storage = crate::runtime_storage::sample(state.database_directory.clone()).await;
    let (pressure, lowest_available_bytes) = crate::runtime_storage::tightest(&storage);
    HiveHealthReport {
        schema_version: HIVE_HEALTH_SCHEMA_VERSION,
        observed_at: now,
        sync: ReportedSync {
            condition: sync.condition,
            failed_step: sync.failed_step,
            last_success_at: sync.last_success_at,
            consecutive_failures: sync.consecutive_failures,
        },
        engine: ReportedEngine {
            update_pending,
            last_attempt,
        },
        disk: ReportedDisk {
            pressure,
            lowest_available_bytes,
        },
        memory: crate::runtime::memory_pressure_now(),
        stuck_workers: stuck_workers(store, now),
        ci_unreadable: store.hive_wide_ci_problem().ok().flatten(),
    }
}

/// Workers holding work they have not moved in half an hour, or were never
/// briefed on: the coordinator's own findings (ADR 0039), counted per worker.
fn stuck_workers(store: &TaskStore, now: i64) -> u32 {
    let Ok(attention) = store.current_coordinator_attention(now) else {
        return 0;
    };
    let workers = attention
        .iter()
        .filter(|item| {
            matches!(
                item.kind.as_str(),
                "stale_owned_work_attention" | "owned_work_never_briefed_attention"
            )
        })
        .map(|item| item.worker_id)
        .collect::<std::collections::HashSet<_>>();
    u32::try_from(workers.len()).unwrap_or(u32::MAX)
}

/// A member's health, received by its Keeper.
pub(crate) async fn accept_member_health(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(report): Json<HiveHealthReport>,
) -> Result<Response, ApiError> {
    let credential = federation_node_credential(&headers)?;
    report.validate().map_err(|reason| {
        ApiError::new(StatusCode::BAD_REQUEST, "invalid_health_report", reason)
    })?;
    let service = apiary_service(&state)?;
    let now = unix_timestamp();
    service
        .accept_hive_health(credential, &report, now)
        .map_err(|_| {
            ApiError::new(
                StatusCode::UNAUTHORIZED,
                "invalid_federation_credential",
                "a current federation node credential is required",
            )
        })?;
    // The moment a fault can have started or cleared. A failure to raise is
    // the Keeper's own problem, never a reason to refuse the member's report.
    if let Err(error) = service.raise_hive_faults(now, build_version()) {
        tracing::warn!(%error, "a member's faults could not be raised or withdrawn");
    }
    state.control_room_notify.notify_waiters();
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        StatusCode::NO_CONTENT,
    )
        .into_response())
}

#[derive(Serialize)]
struct HiveHealthView {
    hive_id: HiveId,
    /// Worst first. Never empty: a Hive with nothing wrong is absent.
    conditions: Vec<HiveCondition>,
}

/// What is wrong with each member Hive. A healthy Hive is absent.
pub(crate) async fn fleet_health(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let service = apiary_service(&state)?;
    let now = unix_timestamp();
    // Read as the roster refreshes, which is also when a Hive that left the
    // Apiary stops being one whose faults are raised.
    service
        .raise_hive_faults(now, build_version())
        .map_err(application_error)?;
    let fleet: Vec<HiveHealthView> = service
        .fleet_health(now, build_version())
        .map_err(application_error)?
        .into_iter()
        .map(|(hive_id, conditions)| HiveHealthView {
            hive_id,
            conditions,
        })
        .collect();
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(fleet)).into_response())
}

#[derive(Serialize)]
struct ContactHistoryView {
    slot_seconds: i64,
    /// The first slot the history covers, and the one now falls in.
    from: i64,
    until: i64,
    /// The starts of the slots in which the Hive reached the Keeper.
    contacted: Vec<i64>,
}

/// When one member reached this Keeper over the last day.
pub(crate) async fn contact_history(
    State(state): State<Arc<AppState>>,
    Path(hive_id): Path<HiveId>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let now = unix_timestamp();
    let contacted = apiary_service(&state)?
        .member_contact_history(hive_id, now)
        .map_err(application_error)?;
    let until = contact_slot(now);
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(ContactHistoryView {
            slot_seconds: CONTACT_SLOT_SECONDS,
            from: until - CONTACT_HISTORY_SECONDS + CONTACT_SLOT_SECONDS,
            until,
            contacted,
        }),
    )
        .into_response())
}

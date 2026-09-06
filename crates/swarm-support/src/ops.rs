//! Ordinary Ops discovery contains no conversations, identity, or diagnostic payloads.
use std::path::PathBuf;

use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};

use crate::{AdminCredential, AppState, failure, now_millis};

#[derive(Clone)]
pub struct OpsConfiguration {
    pub(crate) credential: AdminCredential,
    environment: &'static str,
    build_sha: Option<String>,
    database: PathBuf,
    minimum_free_mib: u32,
}

impl OpsConfiguration {
    /// Configures discovery only; this never registers a source or grants customer sends.
    ///
    /// # Errors
    /// Refuses invalid deployment metadata, credential, or disk threshold.
    pub fn new(
        token: &str,
        environment: &str,
        build_sha: Option<String>,
        database: PathBuf,
        minimum_free_mib: u32,
    ) -> Result<Self, &'static str> {
        let environment = match environment {
            "production" => "production",
            "staging" => "staging",
            "development" => "development",
            _ => return Err("support Ops environment must be explicitly configured"),
        };
        if build_sha.as_ref().is_some_and(|sha| {
            !(7..=64).contains(&sha.len()) || !sha.bytes().all(|b| b.is_ascii_hexdigit())
        }) {
            return Err("support Ops build SHA must be a hexadecimal revision");
        }
        if minimum_free_mib == 0 || !database.is_absolute() {
            return Err(
                "support Ops requires an absolute database path and positive disk threshold",
            );
        }
        Ok(Self {
            credential: AdminCredential::new(token)?,
            environment,
            build_sha,
            database,
            minimum_free_mib,
        })
    }
}

fn authorized<'a>(state: &'a AppState, headers: &HeaderMap) -> Option<&'a OpsConfiguration> {
    state
        .ops
        .as_ref()
        .filter(|ops| ops.credential.accepts(headers))
}

pub(super) async fn manifest(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let Some(ops) = authorized(&state, &headers) else {
        return failure(StatusCode::UNAUTHORIZED, "support_ops_reader_required");
    };
    Json(json!({
        "appId":"swarm-support", "displayName":"Swarm Support", "contractVersion":1,
        "buildSha":ops.build_sha, "environment":ops.environment, "capabilities":[],
        "operatorResources": if state.admin.is_some() { vec!["conversations"] } else { vec![] },
    }))
    .into_response()
}

pub(super) async fn metrics(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if authorized(&state, &headers).is_none() {
        return failure(StatusCode::UNAUTHORIZED, "support_ops_reader_required");
    }
    let Some(now) = now_millis() else {
        return failure(StatusCode::SERVICE_UNAVAILABLE, "support_clock_unavailable");
    };
    Json(json!({"collectedAt":now, "metrics":[]})).into_response()
}

pub(super) async fn incidents(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if authorized(&state, &headers).is_none() {
        return failure(StatusCode::UNAUTHORIZED, "support_ops_reader_required");
    }
    // No incident-history capability exists yet. Health failures remain in health.
    Json(json!({"incidents":[]})).into_response()
}

#[cfg(unix)]
fn available_mib(database: &std::path::Path) -> Option<u64> {
    let stats = nix::sys::statvfs::statvfs(database).ok()?;
    Some(
        stats
            .blocks_available()
            .saturating_mul(stats.fragment_size())
            / (1024 * 1024),
    )
}

#[cfg(not(unix))]
fn available_mib(_database: &std::path::Path) -> Option<u64> {
    None
}

fn health_body(database_ok: bool, free_mib: Option<u64>, minimum_free_mib: u32, now: i64) -> Value {
    let disk_status = match free_mib {
        Some(free) if free >= u64::from(minimum_free_mib) => "healthy",
        Some(_) => "unhealthy",
        None => "degraded",
    };
    let database_status = if database_ok { "healthy" } else { "unhealthy" };
    let status = if database_status == "unhealthy" || disk_status == "unhealthy" {
        "unhealthy"
    } else {
        disk_status
    };
    let mut disk = json!({"name":"support-storage-space", "kind":"disk", "status":disk_status,
        "threshold":minimum_free_mib, "unit":"MiB",
        "detail":if free_mib.is_some() { "Available space on the support database filesystem" } else { "Disk observation unavailable on this platform or filesystem" }});
    if let Some(free) = free_mib {
        disk["observed"] = json!(free);
    }
    json!({"status":status, "checkedAt":now, "checks":[
        {"name":"support-durable-store", "kind":"database", "status":database_status,
         "detail":if database_ok { "Durable read/write probe committed successfully" } else { "Durable read/write probe failed" }}, disk,
    ]})
}

pub(super) async fn health(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Extension(permit): axum::Extension<std::sync::Arc<tokio::sync::OwnedSemaphorePermit>>,
) -> Response {
    let Some(ops) = authorized(&state, &headers).cloned() else {
        return failure(StatusCode::UNAUTHORIZED, "support_ops_reader_required");
    };
    let Some(now) = now_millis() else {
        return failure(StatusCode::SERVICE_UNAVAILABLE, "support_clock_unavailable");
    };
    match tokio::task::spawn_blocking(move || {
        let _permit = permit;
        health_body(
            state.service.check_health().is_ok(),
            available_mib(&ops.database),
            ops.minimum_free_mib,
            now,
        )
    })
    .await
    {
        Ok(body) => Json(body).into_response(),
        Err(_) => failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "support_health_unavailable",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn health_fails_on_storage_or_disk_failure_and_does_not_invent_observations() {
        assert_eq!(health_body(true, Some(128), 64, 1)["status"], "healthy");
        assert_eq!(health_body(false, Some(128), 64, 1)["status"], "unhealthy");
        assert_eq!(health_body(true, Some(63), 64, 1)["status"], "unhealthy");
        let unavailable = health_body(true, None, 64, 1);
        assert_eq!(unavailable["status"], "degraded");
        assert!(unavailable["checks"][1].get("observed").is_none());
    }
}

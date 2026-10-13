//! The Keeper setting what its Apiary requires, and seeing where it is not in
//! effect (ADR 0113).

use std::sync::Arc;

use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use swarm_domain::{
    ApiaryPolicySetting, FederationChangeKind, HiveId, PolicyGap, RequiredSetting, RequiredValue,
};

use crate::{
    ApiError, AppState, announce_federation_change, apiary_service, application_error, authorize,
    unix_timestamp,
};

#[derive(Serialize)]
struct KeeperPolicyView {
    revision: u64,
    /// What the Apiary requires now. A setting absent here is each Hive's own.
    settings: Vec<ApiaryPolicySetting>,
    /// What can be required, and the values each accepts.
    supported: Vec<SupportedSetting>,
}

#[derive(Serialize)]
struct SupportedSetting {
    key: &'static str,
    /// "choice" picks from `values`; "version" takes a release number; "hours"
    /// takes a daily window such as `22:00-07:00`.
    kind: &'static str,
    values: &'static [&'static str],
}

#[derive(Deserialize)]
pub(crate) struct RequirePolicyRequest {
    settings: Vec<ApiaryPolicySetting>,
}

#[derive(Serialize)]
struct HivePolicyGaps {
    hive_id: HiveId,
    gaps: Vec<PolicyGap>,
}

fn view(state: &AppState) -> Result<KeeperPolicyView, ApiError> {
    let Some((revision, settings)) = apiary_service(state)?
        .keeper_policy()
        .map_err(application_error)?
    else {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "not_a_keeper",
            "only an Apiary's Keeper sets what it requires",
        ));
    };
    Ok(KeeperPolicyView {
        revision,
        settings,
        supported: RequiredSetting::ALL
            .iter()
            .map(|setting| SupportedSetting {
                key: setting.key(),
                kind: match setting.value() {
                    RequiredValue::Choice(_) => "choice",
                    RequiredValue::Release => "version",
                    RequiredValue::DailyHours => "hours",
                },
                values: match setting.value() {
                    RequiredValue::Choice(values) => values,
                    RequiredValue::Release | RequiredValue::DailyHours => &[],
                },
            })
            .collect(),
    })
}

/// What this Keeper's Apiary requires.
pub(crate) async fn keeper_policy(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(view(&state)?)).into_response())
}

/// The Keeper replacing what its Apiary requires.
///
/// ⚠️ ONLY SETTINGS MEMBERS APPLY. Under "always required" (ADR 0113) a key no
/// member acts on would read as a requirement and change nothing, so it is
/// refused here, by name, rather than stored.
pub(crate) async fn require_policy(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<RequirePolicyRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    swarm_domain::validate_required_settings(&request.settings).map_err(|reason| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_policy_setting",
            reason,
        )
    })?;
    apiary_service(&state)?
        .require_policy_settings(&request.settings, unix_timestamp())
        .map_err(application_error)?;
    // Rung, so members take it up within a second rather than at their next
    // paced pass.
    announce_federation_change(&state, FederationChangeKind::Policy);
    state.control_room_notify.notify_waiters();
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(view(&state)?)).into_response())
}

/// The member Hives where something required is not in effect, and why.
/// A compliant Hive is absent.
pub(crate) async fn policy_gaps(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let gaps: Vec<HivePolicyGaps> = apiary_service(&state)?
        .policy_gaps()
        .map_err(application_error)?
        .into_iter()
        .map(|(hive_id, gaps)| HivePolicyGaps { hive_id, gaps })
        .collect();
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(gaps)).into_response())
}

/// What this member Hive's Apiary requires of it, for its own screens.
///
/// Empty for a Hive outside an Apiary, or one whose Apiary requires nothing.
pub(crate) async fn member_requirements(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let settings = apiary_service(&state)?
        .required_settings()
        .map_err(application_error)?;
    // Said on this Hive's own screen, so its operator learns it here rather
    // than from a claim the Keeper turns down.
    let shared_work_refusal =
        swarm_domain::minimum_shared_work_version(&settings).and_then(|minimum| {
            swarm_domain::shared_work_refusal(&minimum, Some(crate::build_version()))
        });
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(
            serde_json::json!({ "settings": settings, "shared_work_refusal": shared_work_refusal }),
        ),
    )
        .into_response())
}

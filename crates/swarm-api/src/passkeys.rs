//! Signing in with a passkey.
//!
//! The operator's ruling, 2026-08-23: one passkey and the token as fallback, no
//! separate recovery mechanism. A lost device is recovered with the token, which
//! is a thing they already have rather than another secret to store — and a
//! stale secret in a password manager is what cost them an hour that morning.
//!
//! A credential is bound to one relying-party ID, which is the domain the
//! browser was at when it was registered. This Hive answers on localhost and on
//! a public host; localhost needs no credential at all, so passkeys exist for
//! the public one and the domain is stored with each credential so the wrong
//! ones are never offered.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use webauthn_rs::prelude::{
    CreationChallengeResponse, Passkey, PasskeyAuthentication, PasskeyRegistration,
    PublicKeyCredential, RegisterPublicKeyCredential, RequestChallengeResponse, Url, Uuid,
    Webauthn, WebauthnBuilder,
};

use crate::auth::{authorize, browser_session_cookie};
use crate::{ApiError, AppState};

/// The operator is one person and this is their Hive, so the `WebAuthn` user is
/// a fixed identity rather than a lookup. Per-person identity was settled
/// separately and deliberately not built.
const OPERATOR_HANDLE: Uuid = Uuid::from_u128(0x5761_726d_0000_0000_0000_0000_0000_0001);
const OPERATOR_NAME: &str = "operator";

/// In-flight challenges, held only between a start and its finish.
///
/// Not persisted: a challenge that outlives a restart is a challenge nobody is
/// waiting on, and losing it costs one button press.
#[derive(Default)]
pub(super) struct PasskeyChallenges {
    registrations: PendingChallenges<PasskeyRegistration>,
    authentications: PendingChallenges<PasskeyAuthentication>,
}

const MAX_PENDING_CHALLENGES: usize = 128;
const CHALLENGE_LIFETIME: Duration = Duration::from_secs(300);

/// The API owns abandoned ceremonies as well as completed ones. Expiry is
/// checked on access, without a timer or background task. Full stores refuse
/// new challenges instead of evicting an operator's ceremony in progress.
struct PendingChallenges<T> {
    entries: std::sync::Mutex<HashMap<String, (Instant, T)>>,
}

impl<T> Default for PendingChallenges<T> {
    fn default() -> Self {
        Self {
            entries: std::sync::Mutex::new(HashMap::new()),
        }
    }
}

impl<T> PendingChallenges<T> {
    fn insert(&self, id: String, value: T, now: Instant) -> Result<(), ApiError> {
        let mut entries = self
            .entries
            .lock()
            .map_err(|_| challenge_store_unavailable())?;
        entries.retain(|_, (expires, _)| *expires > now);
        if entries.len() >= MAX_PENDING_CHALLENGES || entries.contains_key(&id) {
            return Err(ApiError::new(
                StatusCode::TOO_MANY_REQUESTS,
                "passkey_challenge_capacity",
                "too many passkey attempts are in progress — wait a few minutes or use your token",
            ));
        }
        entries.insert(id, (now + CHALLENGE_LIFETIME, value));
        Ok(())
    }

    fn take(&self, id: &str, now: Instant) -> Result<Option<T>, ApiError> {
        let mut entries = self
            .entries
            .lock()
            .map_err(|_| challenge_store_unavailable())?;
        entries.retain(|_, (expires, _)| *expires > now);
        Ok(entries.remove(id).map(|(_, value)| value))
    }
}

fn challenge_store_unavailable() -> ApiError {
    ApiError::new(
        StatusCode::SERVICE_UNAVAILABLE,
        "passkey_challenge_store_unavailable",
        "passkey attempts are temporarily unavailable — use your token to sign in",
    )
}

/// The domain a credential belongs to, taken from where the browser actually is.
///
/// A forwarded host wins, because that is what the browser saw; this Hive is
/// published through a tunnel and the socket's own host is not the origin the
/// credential was created against.
fn relying_party(headers: &HeaderMap) -> Option<(String, Url)> {
    let host = headers
        .get("x-forwarded-host")
        .or_else(|| headers.get(header::HOST))
        .and_then(|value| value.to_str().ok())?
        .to_owned();
    let name = host
        .rsplit_once(':')
        .map_or(host.as_str(), |(name, _)| name);
    let scheme = headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .unwrap_or(if name == "localhost" || name == "127.0.0.1" {
            "http"
        } else {
            "https"
        });
    let origin = Url::parse(&format!("{scheme}://{host}")).ok()?;
    Some((name.to_owned(), origin))
}

fn webauthn(headers: &HeaderMap) -> Result<(String, Webauthn), ApiError> {
    let (party, origin) = relying_party(headers).ok_or_else(|| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "passkey_origin_unknown",
            "this request does not say which address the browser is at",
        )
    })?;
    let webauthn = WebauthnBuilder::new(&party, &origin)
        .and_then(WebauthnBuilder::build)
        .map_err(|_| {
            ApiError::new(
                StatusCode::BAD_REQUEST,
                "passkey_origin_unsupported",
                "passkeys cannot be used at this address",
            )
        })?;
    Ok((party, webauthn))
}

#[derive(Debug, Deserialize)]
pub(super) struct RegisterStartRequest {
    label: String,
}

#[derive(Debug, Serialize)]
pub(super) struct RegisterStartResponse {
    challenge_id: String,
    options: CreationChallengeResponse,
}

/// Begins registering a passkey for the address the browser is at.
pub(super) async fn register_start(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<RegisterStartRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let label = request.label.trim();
    if label.is_empty() || label.len() > 120 {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "passkey_label_required",
            "give this passkey a name you will recognise later",
        ));
    }
    let (party, webauthn) = webauthn(&headers)?;
    let existing: Vec<_> = crate::task_store(&state)?
        .operator_passkeys_for(&party)
        .map_err(|error| crate::task_store_error(&error))?
        .into_iter()
        .filter_map(|(_, credential)| serde_json::from_str::<Passkey>(&credential).ok())
        .map(|key| key.cred_id().clone())
        .collect();
    let (options, registration) = webauthn
        .start_passkey_registration(
            OPERATOR_HANDLE,
            OPERATOR_NAME,
            OPERATOR_NAME,
            Some(existing),
        )
        .map_err(|_| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "passkey_registration_unavailable",
                "this passkey registration could not be started",
            )
        })?;
    let challenge_id = Uuid::new_v4().to_string();
    state.passkey_challenges.registrations.insert(
        challenge_id.clone(),
        registration,
        Instant::now(),
    )?;
    Ok(Json(RegisterStartResponse {
        challenge_id,
        options,
    })
    .into_response())
}

#[derive(Debug, Deserialize)]
pub(super) struct RegisterFinishRequest {
    challenge_id: String,
    label: String,
    credential: RegisterPublicKeyCredential,
}

/// Completes registration, storing the credential against this domain.
pub(super) async fn register_finish(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<RegisterFinishRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let (party, webauthn) = webauthn(&headers)?;
    let registration = state
        .passkey_challenges
        .registrations
        .take(&request.challenge_id, Instant::now())?
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::CONFLICT,
                "passkey_challenge_expired",
                "that registration is no longer in progress — start again",
            )
        })?;
    let passkey = webauthn
        .finish_passkey_registration(&request.credential, &registration)
        .map_err(|_| {
            ApiError::new(
                StatusCode::BAD_REQUEST,
                "passkey_registration_rejected",
                "this passkey could not be verified",
            )
        })?;
    let credential_id = encode_credential_id(passkey.cred_id().as_ref());
    let stored = serde_json::to_string(&passkey).map_err(|_| {
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "passkey_not_stored",
            "this passkey could not be stored",
        )
    })?;
    crate::task_store(&state)?
        .add_operator_passkey(
            &credential_id,
            &party,
            request.label.trim(),
            &stored,
            crate::unix_timestamp(),
        )
        .map_err(|error| crate::task_store_error(&error))?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[derive(Debug, Serialize)]
pub(super) struct AuthenticateStartResponse {
    challenge_id: String,
    options: RequestChallengeResponse,
}

/// Begins a passkey sign-in. Deliberately unauthenticated — this is the door.
pub(super) async fn authenticate_start(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let (party, webauthn) = webauthn(&headers)?;
    let keys: Vec<Passkey> = crate::task_store(&state)?
        .operator_passkeys_for(&party)
        .map_err(|error| crate::task_store_error(&error))?
        .into_iter()
        .filter_map(|(_, credential)| serde_json::from_str(&credential).ok())
        .collect();
    if keys.is_empty() {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "no_passkey_registered",
            "no passkey is registered for this address",
        ));
    }
    let (options, authentication) = webauthn.start_passkey_authentication(&keys).map_err(|_| {
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "passkey_challenge_unavailable",
            "a passkey sign-in could not be started",
        )
    })?;
    let challenge_id = Uuid::new_v4().to_string();
    state.passkey_challenges.authentications.insert(
        challenge_id.clone(),
        authentication,
        Instant::now(),
    )?;
    Ok(Json(AuthenticateStartResponse {
        challenge_id,
        options,
    })
    .into_response())
}

#[derive(Debug, Deserialize)]
pub(super) struct AuthenticateFinishRequest {
    challenge_id: String,
    credential: PublicKeyCredential,
}

/// Completes a passkey sign-in by minting the same session cookie a token does.
///
/// One session model: everything downstream already understands that cookie,
/// and a second kind of credential would be a second thing to expire, revoke
/// and reason about.
pub(super) async fn authenticate_finish(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<AuthenticateFinishRequest>,
) -> Result<Response, ApiError> {
    let (_, webauthn) = webauthn(&headers)?;
    let authentication = state
        .passkey_challenges
        .authentications
        .take(&request.challenge_id, Instant::now())?
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::CONFLICT,
                "passkey_challenge_expired",
                "that sign-in is no longer in progress — try again",
            )
        })?;
    let result = webauthn
        .finish_passkey_authentication(&request.credential, &authentication)
        .map_err(|_| {
            ApiError::new(
                StatusCode::UNAUTHORIZED,
                "passkey_rejected",
                "that passkey was not accepted",
            )
        })?;
    // The counter moves with each use, so writing the credential back is what
    // keeps a cloned authenticator detectable.
    if result.needs_update() {
        let credential_id = encode_credential_id(result.cred_id().as_ref());
        if let Ok(store) = crate::task_store(&state) {
            let _ = refresh_stored_counter(store, &credential_id, &result);
        }
    }
    let cookie = browser_session_cookie(&state, &headers)?;
    Ok((
        StatusCode::NO_CONTENT,
        [(header::SET_COOKIE, cookie)],
        [(header::CACHE_CONTROL, "no-store")],
    )
        .into_response())
}

/// Writes the credential back after a successful sign-in.
///
/// The signature counter moves with each use, and storing the new value is what
/// keeps a cloned authenticator detectable — a replayed counter is the only
/// signal there is.
fn refresh_stored_counter(
    store: &swarm_persistence::TaskStore,
    credential_id: &str,
    result: &webauthn_rs::prelude::AuthenticationResult,
) -> Result<(), swarm_persistence::TaskStoreError> {
    let Some(key) = store
        .operator_passkeys()?
        .into_iter()
        .find(|key| key.credential_id == credential_id)
    else {
        return Ok(());
    };
    let Some((_, credential)) = store
        .operator_passkeys_for(&key.relying_party)?
        .into_iter()
        .find(|(stored, _)| stored.credential_id == credential_id)
    else {
        return Ok(());
    };
    let Ok(mut passkey) = serde_json::from_str::<Passkey>(&credential) else {
        return Ok(());
    };
    passkey.update_credential(result);
    if let Ok(updated) = serde_json::to_string(&passkey) {
        store.record_passkey_use(credential_id, &updated, crate::unix_timestamp())?;
    }
    Ok(())
}

#[derive(Debug, Serialize)]
pub(super) struct PasskeyResponse {
    credential_id: String,
    relying_party: String,
    label: String,
    created_at: i64,
    last_used_at: Option<i64>,
    /// Whether this credential belongs to the address the browser is at. One
    /// registered elsewhere cannot sign in here, and saying so is kinder than
    /// letting the browser fail.
    usable_here: bool,
}

pub(super) async fn list_passkeys(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let here = relying_party(&headers).map(|(party, _)| party);
    let keys = crate::task_store(&state)?
        .operator_passkeys()
        .map_err(|error| crate::task_store_error(&error))?
        .into_iter()
        .map(|key| PasskeyResponse {
            usable_here: here.as_deref() == Some(key.relying_party.as_str()),
            credential_id: key.credential_id,
            relying_party: key.relying_party,
            label: key.label,
            created_at: key.created_at,
            last_used_at: key.last_used_at,
        })
        .collect::<Vec<_>>();
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(keys)).into_response())
}

pub(super) async fn remove_passkey(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(credential_id): Path<String>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let removed = crate::task_store(&state)?
        .remove_operator_passkey(&credential_id)
        .map_err(|error| crate::task_store_error(&error))?;
    if removed {
        Ok(StatusCode::NO_CONTENT.into_response())
    } else {
        Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "passkey_not_found",
            "that passkey is not registered here",
        ))
    }
}

fn encode_credential_id(bytes: &[u8]) -> String {
    use base64ct::{Base64UrlUnpadded, Encoding as _};
    Base64UrlUnpadded::encode_string(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn registration_start_refuses_capacity_and_recovers_without_restart() {
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt as _;

        let state =
            AppState::default().with_task_store(swarm_persistence::TaskStore::in_memory().unwrap());
        *state.operator_token.write().unwrap() = Some("fictional-passkey-test".into());
        let challenges = Arc::clone(&state.passkey_challenges);
        let app = crate::router(state);
        let request = || {
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/passkeys/register/start")
                .header("host", "swarm.example.com")
                .header("authorization", "Bearer fictional-passkey-test")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"label":"Fictional test"}"#))
                .unwrap()
        };
        for _ in 0..MAX_PENDING_CHALLENGES {
            assert_eq!(
                app.clone().oneshot(request()).await.unwrap().status(),
                StatusCode::OK
            );
        }
        assert_eq!(
            app.clone().oneshot(request()).await.unwrap().status(),
            StatusCode::TOO_MANY_REQUESTS
        );
        // Explicit fixture time, not a sleep: all abandoned ceremonies have expired.
        for (expires, _) in challenges
            .registrations
            .entries
            .lock()
            .unwrap()
            .values_mut()
        {
            *expires = Instant::now();
        }
        assert_eq!(
            app.oneshot(request()).await.unwrap().status(),
            StatusCode::OK
        );
        assert_eq!(challenges.registrations.entries.lock().unwrap().len(), 1);
    }

    #[test]
    fn abandoned_challenges_are_bounded_and_expiry_restores_capacity() {
        let pending = PendingChallenges::default();
        let now = Instant::now();
        for id in 0..MAX_PENDING_CHALLENGES {
            pending.insert(id.to_string(), id, now).unwrap();
        }
        assert!(pending.insert("overflow".into(), 999, now).is_err());
        assert_eq!(
            pending.entries.lock().unwrap().len(),
            MAX_PENDING_CHALLENGES
        );
        // Refusal has not displaced an existing ceremony; consumption frees room.
        assert_eq!(pending.take("0", now).unwrap(), Some(0));
        pending.insert("replacement".into(), 1000, now).unwrap();
        pending
            .insert("after-expiry".into(), 1001, now + CHALLENGE_LIFETIME)
            .unwrap();
        assert_eq!(pending.entries.lock().unwrap().len(), 1);
        assert_eq!(
            pending
                .take("replacement", now + CHALLENGE_LIFETIME)
                .unwrap(),
            None
        );
        assert_eq!(
            pending
                .take("after-expiry", now + CHALLENGE_LIFETIME)
                .unwrap(),
            Some(1001)
        );
    }

    #[test]
    fn challenges_are_one_time_and_expire_at_the_boundary() {
        let pending = PendingChallenges::default();
        let now = Instant::now();
        pending.insert("once".into(), 1, now).unwrap();
        assert_eq!(pending.take("once", now).unwrap(), Some(1));
        assert_eq!(pending.take("once", now).unwrap(), None);
        pending.insert("expired".into(), 2, now).unwrap();
        assert_eq!(
            pending.take("expired", now + CHALLENGE_LIFETIME).unwrap(),
            None
        );
        assert!(pending.entries.lock().unwrap().is_empty());
    }

    #[test]
    fn duplicate_identity_cannot_replace_a_pending_ceremony() {
        let pending = PendingChallenges::default();
        let now = Instant::now();
        pending.insert("same".into(), 1, now).unwrap();
        assert!(pending.insert("same".into(), 2, now).is_err());
        assert_eq!(pending.take("same", now).unwrap(), Some(1));
    }

    #[test]
    fn poisoned_store_reports_failure_instead_of_an_unusable_challenge() {
        let pending = Arc::new(PendingChallenges::default());
        let other = Arc::clone(&pending);
        assert!(
            std::thread::spawn(move || {
                let _guard = other.entries.lock().unwrap();
                panic!("fictional store failure");
            })
            .join()
            .is_err()
        );
        assert!(
            pending
                .insert("unavailable".into(), 1, Instant::now())
                .is_err()
        );
        assert!(pending.take("unavailable", Instant::now()).is_err());
    }
}

use std::{str::FromStr, sync::Arc, time::Duration};

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use base64ct::{Base64UrlUnpadded, Encoding};
use serde::{Deserialize, Serialize};
use swarm_domain::{DecisionUrgency, NotificationPolicy, PresenceDeviceClass, PresenceDeviceId};
use swarm_persistence::{
    NotificationDeliveryFailure, NotificationDispatch, NotificationSettings, PushSubscriptionInput,
    TaskStore,
};
use tokio::sync::Mutex;
use web_push_native::{
    Auth, WebPushBuilder,
    jwt_simple::algorithms::ES256KeyPair,
    p256::{PublicKey, elliptic_curve::sec1::ToEncodedPoint},
};

use super::{ApiError, AppState, authorize, task_store, task_store_error};

const PUSH_TIMEOUT: Duration = Duration::from_secs(10);
const PUSH_TTL: Duration = Duration::from_secs(60 * 60);
const MAX_PUSH_PAYLOAD_BYTES: usize = 512;

#[derive(Clone)]
pub struct NotificationSender {
    store: TaskStore,
    client: reqwest::Client,
    key_pair: Arc<ES256KeyPair>,
    public_key: Arc<str>,
    subject: Arc<str>,
    delivery_lock: Arc<Mutex<()>>,
}

impl NotificationSender {
    /// Creates or restores the installation key and one bounded shared HTTPS client.
    ///
    /// # Errors
    /// Returns key, storage, or HTTPS client initialization failures.
    pub fn initialize(store: TaskStore, subject: impl Into<Arc<str>>) -> Result<Self, String> {
        let generated = ES256KeyPair::generate();
        let private_key = generated.to_bytes();
        let compressed_public_key = generated.public_key().to_bytes();
        let public_key = PublicKey::from_sec1_bytes(&compressed_public_key)
            .map_err(|_| "generated notification public key is invalid".to_owned())?
            .to_encoded_point(false);
        let material = store
            .ensure_vapid_key(&private_key, public_key.as_bytes())
            .map_err(|error| error.to_string())?;
        let key_pair =
            ES256KeyPair::from_bytes(&material.private_key).map_err(|error| error.to_string())?;
        let restored_public_key = PublicKey::from_sec1_bytes(&key_pair.public_key().to_bytes())
            .map_err(|_| "stored notification public key is invalid".to_owned())?
            .to_encoded_point(false);
        if restored_public_key.as_bytes() != material.public_key {
            return Err("stored notification key pair does not match".to_owned());
        }
        let public_key = Base64UrlUnpadded::encode_string(&material.public_key);
        let client = reqwest::Client::builder()
            .timeout(PUSH_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| error.to_string())?;
        Ok(Self {
            store,
            client,
            key_pair: Arc::new(key_pair),
            public_key: public_key.into(),
            subject: subject.into(),
            delivery_lock: Arc::new(Mutex::new(())),
        })
    }

    #[must_use]
    pub fn public_key(&self) -> &str {
        &self.public_key
    }

    /// Sends one currently eligible bounded batch. Durable queue state owns retries.
    pub async fn deliver(&self) {
        let _guard = self.delivery_lock.lock().await;
        let deliveries = match self.store.claim_notification_deliveries(unix_timestamp()) {
            Ok(value) => value,
            Err(error) => {
                tracing::warn!(message = %error, "notification queue could not be claimed");
                return;
            }
        };
        for delivery in deliveries {
            let result = self.send(&delivery).await;
            let persisted = match result {
                Ok(()) => self
                    .store
                    .complete_notification_delivery(delivery.delivery_id, delivery.subscription_id),
                Err(failure) => self.store.fail_notification_delivery(
                    delivery.delivery_id,
                    delivery.subscription_id,
                    failure,
                    unix_timestamp(),
                ),
            };
            if let Err(error) = persisted {
                tracing::warn!(delivery_id = delivery.delivery_id, message = %error, "notification delivery outcome could not be persisted");
            }
        }
    }

    async fn send(
        &self,
        delivery: &NotificationDispatch,
    ) -> Result<(), NotificationDeliveryFailure> {
        validate_push_endpoint(&delivery.endpoint)
            .map_err(|_| NotificationDeliveryFailure::Permanent)?;
        let endpoint = delivery
            .endpoint
            .parse()
            .map_err(|_| NotificationDeliveryFailure::Permanent)?;
        let public_key = PublicKey::from_sec1_bytes(&delivery.p256dh)
            .map_err(|_| NotificationDeliveryFailure::Permanent)?;
        let auth = Auth::clone_from_slice(&delivery.auth);
        let payload = notification_payload(delivery);
        if payload.len() > MAX_PUSH_PAYLOAD_BYTES {
            return Err(NotificationDeliveryFailure::Permanent);
        }
        let request = WebPushBuilder::new(endpoint, public_key, auth)
            .with_valid_duration(PUSH_TTL)
            .with_vapid(&self.key_pair, &self.subject)
            .build(payload)
            .map_err(|_| NotificationDeliveryFailure::Permanent)?;
        let (parts, body) = request.into_parts();
        let response = self
            .client
            .request(parts.method, parts.uri.to_string())
            .headers(parts.headers)
            .body(body)
            .send()
            .await
            .map_err(|_| NotificationDeliveryFailure::Retryable)?;
        match response.status().as_u16() {
            200..=299 => Ok(()),
            404 | 410 => Err(NotificationDeliveryFailure::Gone),
            408 | 429 | 500..=599 => Err(NotificationDeliveryFailure::Retryable),
            _ => Err(NotificationDeliveryFailure::Permanent),
        }
    }
}

#[derive(Serialize)]
struct PushPayload<'a> {
    title: &'static str,
    body: &'static str,
    tag: String,
    url: &'a str,
    urgency: &'static str,
}

fn notification_payload(delivery: &NotificationDispatch) -> Vec<u8> {
    let (title, body, tag, url) = if delivery.test {
        (
            "Swarm can reach you",
            "Mobile attention is ready for this Hive.",
            format!("swarm-test-{}", delivery.subscription_id),
            "/?surface=settings",
        )
    } else {
        (
            "Your Hive needs you",
            "Open Swarm to review a decision.",
            delivery.decision_id.map_or_else(
                || "swarm-attention".to_owned(),
                |decision_id| format!("swarm-decision-{decision_id}"),
            ),
            "/?surface=decisions",
        )
    };
    serde_json::to_vec(&PushPayload {
        title,
        body,
        tag,
        url,
        urgency: if delivery.urgency == DecisionUrgency::TimeSensitive {
            "time_sensitive"
        } else {
            "normal"
        },
    })
    .expect("fixed notification payload is serializable")
}

pub fn validate_push_endpoint(value: &str) -> Result<(), &'static str> {
    let url = reqwest::Url::parse(value).map_err(|_| "push endpoint must be a URL")?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some_and(|port| port != 443)
        || url.fragment().is_some()
    {
        return Err("push endpoint must use plain HTTPS on the default port");
    }
    let host = url
        .host_str()
        .ok_or("push endpoint must have a trusted host")?
        .to_ascii_lowercase();
    let trusted = host == "fcm.googleapis.com"
        || host == "updates.push.services.mozilla.com"
        || host == "push.services.mozilla.com"
        || host == "web.push.apple.com"
        || host.ends_with(".notify.windows.com");
    if !trusted {
        return Err("push endpoint host is not an approved browser push service");
    }
    Ok(())
}

pub fn validate_subscription_public_key(value: &[u8]) -> Result<(), &'static str> {
    PublicKey::from_sec1_bytes(value)
        .map(|_| ())
        .map_err(|_| "subscription public key is not a valid P-256 point")
}
pub fn decode_subscription_key(value: &str, expected: usize) -> Result<Vec<u8>, &'static str> {
    let decoded =
        Base64UrlUnpadded::decode_vec(value).map_err(|_| "subscription key is not base64url")?;
    if decoded.len() != expected {
        return Err("subscription key has an invalid length");
    }
    Ok(decoded)
}

#[derive(Debug, Deserialize)]
pub(super) struct SetNotificationPolicyRequest {
    policy: NotificationPolicy,
}

#[derive(Debug, Deserialize)]
pub(super) struct SaveNotificationSubscriptionRequest {
    device_class: PresenceDeviceClass,
    endpoint: String,
    keys: NotificationSubscriptionKeys,
}

#[derive(Debug, Deserialize)]
struct NotificationSubscriptionKeys {
    p256dh: String,
    auth: String,
}

#[derive(Debug, Serialize)]
pub(super) struct NotificationSubscriptionStatus {
    registered: bool,
}

pub(super) async fn notification_settings(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let sender = state.notification_sender.as_ref().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "notifications_unavailable",
            "notification transport is unavailable",
        )
    })?;
    let settings = task_store(&state)?
        .notification_settings()
        .map_err(|error| task_store_error(&error))?;
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(NotificationSettingsResponse {
            settings,
            vapid_public_key: sender.public_key(),
        }),
    )
        .into_response())
}

pub(super) async fn set_notification_policy(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<SetNotificationPolicyRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let settings = task_store(&state)?
        .set_notification_policy(request.policy, unix_timestamp())
        .map_err(|error| task_store_error(&error))?;
    state.control_room_notify.notify_waiters();
    schedule_notification_delivery(&state);
    notification_settings_response(&state, settings)
}

pub(super) async fn save_notification_subscription(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(device_id): Path<String>,
    Json(request): Json<SaveNotificationSubscriptionRequest>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    validate_push_endpoint(&request.endpoint).map_err(|message| {
        ApiError::new(StatusCode::BAD_REQUEST, "invalid_push_endpoint", message)
    })?;
    let device_id = parse_notification_device_id(&device_id)?;
    let p256dh = decode_subscription_key(&request.keys.p256dh, 65)
        .map_err(|message| ApiError::new(StatusCode::BAD_REQUEST, "invalid_push_key", message))?;
    validate_subscription_public_key(&p256dh)
        .map_err(|message| ApiError::new(StatusCode::BAD_REQUEST, "invalid_push_key", message))?;
    let auth = decode_subscription_key(&request.keys.auth, 16)
        .map_err(|message| ApiError::new(StatusCode::BAD_REQUEST, "invalid_push_key", message))?;
    let settings = task_store(&state)?
        .save_notification_subscription(
            &PushSubscriptionInput {
                device_id,
                device_class: request.device_class,
                endpoint: request.endpoint,
                p256dh,
                auth,
            },
            unix_timestamp(),
        )
        .map_err(|error| task_store_error(&error))?;
    state.control_room_notify.notify_waiters();
    notification_settings_response(&state, settings)
}

pub(super) async fn notification_subscription_status(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(device_id): Path<String>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let registered = task_store(&state)?
        .has_notification_subscription(parse_notification_device_id(&device_id)?)
        .map_err(|error| task_store_error(&error))?;
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(NotificationSubscriptionStatus { registered }),
    )
        .into_response())
}

pub(super) async fn remove_notification_subscription(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(device_id): Path<String>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let settings = task_store(&state)?
        .remove_notification_subscription(parse_notification_device_id(&device_id)?)
        .map_err(|error| task_store_error(&error))?;
    state.control_room_notify.notify_waiters();
    notification_settings_response(&state, settings)
}

pub(super) async fn test_notification(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(device_id): Path<String>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers)?;
    let queued = task_store(&state)?
        .enqueue_device_test_notification(
            parse_notification_device_id(&device_id)?,
            unix_timestamp(),
        )
        .map_err(|error| task_store_error(&error))?;
    if !queued {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "notification_device_not_found",
            "this browser is not registered for notifications",
        ));
    }
    schedule_notification_delivery(&state);
    let settings = task_store(&state)?
        .notification_settings()
        .map_err(|error| task_store_error(&error))?;
    notification_settings_response(&state, settings)
}

fn parse_notification_device_id(device_id: &str) -> Result<PresenceDeviceId, ApiError> {
    PresenceDeviceId::from_str(device_id).map_err(|_| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_presence_device_id",
            "notification device ID must be a UUID",
        )
    })
}

fn schedule_notification_delivery(state: &Arc<AppState>) {
    let sender = state.notification_sender.clone();
    tokio::spawn(async move {
        if let Some(sender) = sender {
            sender.deliver().await;
        }
    });
}

fn notification_settings_response(
    state: &AppState,
    settings: NotificationSettings,
) -> Result<Response, ApiError> {
    let sender = state.notification_sender.as_ref().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "notifications_unavailable",
            "notification transport is unavailable",
        )
    })?;
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(NotificationSettingsResponse {
            settings,
            vapid_public_key: sender.public_key(),
        }),
    )
        .into_response())
}

#[derive(Serialize)]
pub struct NotificationSettingsResponse<'a> {
    #[serde(flatten)]
    pub settings: NotificationSettings,
    pub vapid_public_key: &'a str,
}

fn unix_timestamp() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| {
            i64::try_from(duration.as_secs()).unwrap_or(i64::MAX)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use swarm_domain::PresenceDeviceId;

    #[test]
    fn endpoint_allowlist_rejects_ssrf_shapes_and_accepts_browser_services() {
        assert!(validate_push_endpoint("https://fcm.googleapis.com/fcm/send/abc").is_ok());
        assert!(
            validate_push_endpoint("https://updates.push.services.mozilla.com/wpush/v2/x").is_ok()
        );
        assert!(
            validate_push_endpoint("https://wns2-par02p.notify.windows.com/w/?token=x").is_ok()
        );
        for rejected in [
            "http://fcm.googleapis.com/x",
            "https://fcm.googleapis.com:444/x",
            "https://fcm.googleapis.com.evil.example/x",
            "https://127.0.0.1/x",
            "https://user@fcm.googleapis.com/x",
        ] {
            assert!(validate_push_endpoint(rejected).is_err(), "{rejected}");
        }
    }

    #[test]
    fn payload_is_generic_content_free_and_stably_tagged() {
        let decision_id = swarm_domain::DecisionRequestId::new();
        let delivery = NotificationDispatch {
            delivery_id: 1,
            subscription_id: PresenceDeviceId::new(),
            endpoint: "https://fcm.googleapis.com/fcm/send/test".into(),
            p256dh: vec![0; 65],
            auth: vec![0; 16],
            decision_id: Some(decision_id),
            urgency: DecisionUrgency::TimeSensitive,
            test: false,
        };
        let payload = String::from_utf8(notification_payload(&delivery)).unwrap();
        assert!(payload.contains("Your Hive needs you"));
        assert!(payload.contains(&format!("swarm-decision-{decision_id}")));
        assert!(!payload.contains("reason"));
        assert!(payload.len() <= MAX_PUSH_PAYLOAD_BYTES);
    }
}

/// What the service worker did when a notification was tapped.
///
/// Notification clicks happen in the service worker, where nothing is visible:
/// no console anyone reads, no page to inspect, and the failure the operator
/// reports is "nothing happens", which is indistinguishable from the handler
/// never running. This is how that becomes observable rather than guessed at —
/// the same reason the terminal size ledger exists.
#[derive(Debug, serde::Deserialize)]
pub(super) struct NotificationClickTrace {
    /// How many same-origin windows the worker could see.
    windows: u32,
    /// How many of those reported themselves visible.
    visible: u32,
    /// What it decided to do: "focus", "open", or "none".
    action: String,
    /// The surface the notification asked for.
    surface: String,
    /// Whatever went wrong, if anything.
    detail: Option<String>,
}

pub(super) async fn record_click_trace(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(trace): Json<NotificationClickTrace>,
) -> Result<StatusCode, ApiError> {
    authorize(&state, &headers)?;
    tracing::info!(
        windows = trace.windows,
        visible = trace.visible,
        action = %trace.action,
        surface = %trace.surface,
        detail = trace.detail.as_deref().unwrap_or(""),
        "notification click"
    );
    Ok(StatusCode::NO_CONTENT)
}

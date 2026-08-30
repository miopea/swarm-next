//! Connecting a person's own GitHub account, without a password or a token.
//!
//! WHY THIS EXISTS. Feedback filed with the Hive's credential is a dead end for
//! the person who wrote it: the issue is authored by the Hive owner, so closing
//! it with "fixed!" reaches nobody. The operator's ruling is that a person who
//! connects their own account files as themselves — and then GitHub's own
//! notifications close the loop with nothing extra built. "They can link their
//! GitHub account to the issue, which would be automatic in theory, and
//! therefore they can get a response."
//!
//! WHY THE DEVICE FLOW AND NOT THE REDIRECT FLOW THE REST OF THIS CRATE USES.
//! `jira_oauth` and `microsoft_oauth` both build a `redirect_uri` from
//! `SWARM_PUBLIC_BASE_URL`, which works because there is one Hive with one stable
//! URL. Swarm is distributed. Every Hive has a different address, and an OAuth
//! callback must be registered in advance, so a redirect flow would force every
//! operator to register their own GitHub App before anybody could connect —
//! worse than the token paste it replaces.
//!
//! The device flow needs no callback and no client secret. The person is shown
//! a short code, types it at github.com/login/device, and this polls until they
//! finish. It behaves identically on a phone, which is where this operator
//! actually reports from.
//!
//! NO SECRET LIVES HERE. The client id is public by design — it ships in every
//! copy of Swarm and identifies the app, not the person. What comes back IS
//! sensitive and never leaves the server.

use serde::Deserialize;

/// The Swarm Feedback GitHub App, registered once against miopea/swarm-next.
///
/// A CONSTANT RATHER THAN CONFIGURATION, deliberately. The operator's
/// requirement was "whatever is easy for the end user"; an environment variable
/// every Hive must set before anyone can connect is the opposite of that, and
/// it is not a secret worth protecting — a client id is published in every
/// OAuth request that uses it. The override exists for a fork or a test app,
/// not for ordinary installs.
pub(super) const DEFAULT_CLIENT_ID: &str = "Iv23lilNUzl8jQb1VhFW";

const DEVICE_CODE_URL: &str = "https://github.com/login/device/code";
const ACCESS_TOKEN_URL: &str = "https://github.com/login/oauth/access_token";

#[derive(Debug, PartialEq, Eq)]
pub(super) enum DeviceError {
    /// GitHub could not be reached at all.
    Unreachable,
    /// GitHub answered, and said no. Carries its own words.
    Refused(String),
}

/// What to show a person so they can authorise this Hive.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub(super) struct DeviceInvitation {
    /// Held by the server and polled with. Never shown; it is the secret half.
    pub(super) device_code: String,
    /// The short code the person types. Shown.
    pub(super) user_code: String,
    /// Where they type it.
    pub(super) verification_uri: String,
    /// Seconds until `user_code` stops working.
    pub(super) expires_in: i64,
    /// Seconds GitHub requires between polls. Ignoring it earns `slow_down`.
    pub(super) interval: i64,
}

/// Where a pending authorisation has got to.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum DeviceOutcome {
    /// They have not finished yet. Keep waiting, at the stated interval.
    Pending,
    /// Polled too fast. The new interval is authoritative.
    SlowDown { interval: i64 },
    /// They said no.
    Denied,
    /// The code aged out before they used it.
    Expired,
    /// Done. These never leave the server.
    Granted(GrantedTokens),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct GrantedTokens {
    pub(super) access_token: String,
    /// Present because the app expires user tokens — the more secure setting,
    /// which costs a refresh implementation and buys no standing credentials.
    pub(super) refresh_token: Option<String>,
    pub(super) expires_in: Option<i64>,
    pub(super) refresh_token_expires_in: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_in: Option<i64>,
    refresh_token_expires_in: Option<i64>,
    error: Option<String>,
    error_description: Option<String>,
    interval: Option<i64>,
}

/// Asks GitHub to start an authorisation and returns what to show the person.
///
/// # Errors
/// Returns `Unreachable` when GitHub cannot be reached, `Refused` when it
/// answers with an error — a client id that is not a real app, or an app whose
/// device flow was never enabled, both land here.
pub(super) async fn invite(client_id: &str) -> Result<DeviceInvitation, DeviceError> {
    // No scope parameter. A GitHub App's permissions are fixed at registration
    // and at install time, so asking for scopes here is an OAuth App idiom that
    // GitHub ignores and a reader would misread as the source of truth.
    let response = reqwest::Client::new()
        .post(DEVICE_CODE_URL)
        .header("accept", "application/json")
        // A JSON body rather than a form one: reqwest is built here without the
        // urlencoded feature, and GitHub accepts either. Confirmed against the
        // live endpoint rather than assumed — a wrong content type here fails
        // at runtime, in the one path that has no test because it needs a
        // network.
        .json(&serde_json::json!({ "client_id": client_id }))
        .send()
        .await
        .map_err(|_| DeviceError::Unreachable)?;
    let body = response
        .text()
        .await
        .map_err(|_| DeviceError::Unreachable)?;
    read_invitation(&body)
}

/// Parsed separately so the shapes GitHub returns can be tested without a
/// network, which is the only part of this that CAN be tested without one.
fn read_invitation(body: &str) -> Result<DeviceInvitation, DeviceError> {
    if let Ok(invitation) = serde_json::from_str::<DeviceInvitation>(body) {
        return Ok(invitation);
    }
    let failure = serde_json::from_str::<TokenResponse>(body)
        .ok()
        .and_then(|parsed| parsed.error_description.or(parsed.error))
        .unwrap_or_else(|| "GitHub returned an unreadable device response".to_owned());
    Err(DeviceError::Refused(failure))
}

/// Asks once whether the person has finished. Call at the stated interval.
///
/// # Errors
/// Returns `Unreachable` when GitHub cannot be reached.
pub(super) async fn claim(
    client_id: &str,
    device_code: &str,
) -> Result<DeviceOutcome, DeviceError> {
    let response = reqwest::Client::new()
        .post(ACCESS_TOKEN_URL)
        .header("accept", "application/json")
        .json(&serde_json::json!({
            "client_id": client_id,
            "device_code": device_code,
            "grant_type": "urn:ietf:params:oauth:grant-type:device_code",
        }))
        .send()
        .await
        .map_err(|_| DeviceError::Unreachable)?;
    let body = response
        .text()
        .await
        .map_err(|_| DeviceError::Unreachable)?;
    read_outcome(&body)
}

/// GitHub answers this endpoint with HTTP 200 and an `error` field, so the
/// status code says nothing and the body is the only signal.
fn read_outcome(body: &str) -> Result<DeviceOutcome, DeviceError> {
    let parsed = serde_json::from_str::<TokenResponse>(body).map_err(|_| {
        DeviceError::Refused("GitHub returned an unreadable token response".to_owned())
    })?;
    if let Some(error) = parsed.error.as_deref() {
        return Ok(match error {
            // NOT FAILURES. Both of these mean "carry on waiting", and treating
            // either as an error ends an authorisation the person is midway
            // through.
            "authorization_pending" => DeviceOutcome::Pending,
            "slow_down" => DeviceOutcome::SlowDown {
                interval: parsed.interval.unwrap_or(10),
            },
            "access_denied" => DeviceOutcome::Denied,
            "expired_token" => DeviceOutcome::Expired,
            other => {
                return Err(DeviceError::Refused(
                    parsed.error_description.unwrap_or_else(|| other.to_owned()),
                ));
            }
        });
    }
    let Some(access_token) = parsed.access_token else {
        return Err(DeviceError::Refused(
            "GitHub reported success without a token".to_owned(),
        ));
    };
    Ok(DeviceOutcome::Granted(GrantedTokens {
        access_token,
        refresh_token: parsed.refresh_token,
        expires_in: parsed.expires_in,
        refresh_token_expires_in: parsed.refresh_token_expires_in,
    }))
}

/// Trades a refresh token for a fresh access token.
///
/// WHY THIS HAS TO EXIST AT ALL. The app expires user tokens — measured from
/// GitHub's own grant rather than read off a settings page: the operator's
/// connection came back with an access token good for eight hours and a refresh
/// token good for six months. Without this, a connection dies overnight and
/// filing silently falls back to anonymous, so the person who connected
/// SPECIFICALLY to hear back stops hearing back and nothing tells them.
///
/// # Errors
/// Returns `Unreachable`, or `Refused` when GitHub declines — a refresh token
/// that has been revoked or has itself expired lands there, and that is a
/// connection which is genuinely over rather than one worth retrying.
pub(super) async fn refresh(
    client_id: &str,
    refresh_token: &str,
) -> Result<GrantedTokens, DeviceError> {
    let response = reqwest::Client::new()
        .post(ACCESS_TOKEN_URL)
        .header("accept", "application/json")
        .json(&serde_json::json!({
            "client_id": client_id,
            "refresh_token": refresh_token,
            "grant_type": "refresh_token",
        }))
        .send()
        .await
        .map_err(|_| DeviceError::Unreachable)?;
    let body = response
        .text()
        .await
        .map_err(|_| DeviceError::Unreachable)?;
    match read_outcome(&body)? {
        DeviceOutcome::Granted(tokens) => Ok(tokens),
        // A refresh does not have a "still waiting" state; anything that is not
        // a grant means this connection is over.
        other => Err(DeviceError::Refused(format!(
            "GitHub would not refresh this connection ({other:?})"
        ))),
    }
}

/// Who a granted token belongs to, so the UI can say which account is connected.
///
/// Asked once at connect time rather than stored from the grant, because the
/// grant does not carry it — and a connection that cannot name its account is
/// one the operator cannot audit or revoke with confidence.
///
/// # Errors
/// Returns `Unreachable` or GitHub's own refusal.
pub(super) async fn whoami(access_token: &str) -> Result<String, DeviceError> {
    let response = reqwest::Client::new()
        .get("https://api.github.com/user")
        .header("authorization", format!("Bearer {access_token}"))
        .header("accept", "application/vnd.github+json")
        .header("user-agent", "swarm-next")
        .send()
        .await
        .map_err(|_| DeviceError::Unreachable)?;
    if !response.status().is_success() {
        return Err(DeviceError::Refused(format!(
            "GitHub refused to identify the account ({})",
            response.status()
        )));
    }
    let body = response
        .text()
        .await
        .map_err(|_| DeviceError::Unreachable)?;
    read_login(&body)
}

fn read_login(body: &str) -> Result<String, DeviceError> {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|parsed| {
            parsed
                .get("login")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .filter(|login| !login.is_empty())
        .ok_or_else(|| DeviceError::Refused("GitHub returned no account login".to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape GitHub actually returned, captured from a live call.
    #[test]
    fn an_invitation_carries_what_the_person_needs_and_what_the_server_polls_with() {
        let invitation = read_invitation(
            r#"{"device_code":"dc-secret","user_code":"2F49-5696",
                "verification_uri":"https://github.com/login/device",
                "expires_in":899,"interval":5}"#,
        )
        .unwrap();

        assert_eq!(invitation.user_code, "2F49-5696");
        assert_eq!(
            invitation.verification_uri,
            "https://github.com/login/device"
        );
        assert_eq!(invitation.interval, 5);
        // The device code is the secret half. It is held, never shown.
        assert_eq!(invitation.device_code, "dc-secret");
    }

    /// An app whose device flow was never enabled fails HERE, and must say so.
    ///
    /// The Enable Device Flow checkbox is off by default when registering a
    /// GitHub App. Getting this wrong produces an app that looks correct in
    /// every screen and cannot authorise anybody, so the refusal has to carry
    /// GitHub's own words rather than a generic failure.
    #[test]
    fn an_app_without_device_flow_is_refused_in_githubs_own_words() {
        let refused = read_invitation(
            r#"{"error":"device_flow_disabled",
                "error_description":"Device Flow has not been enabled for this app."}"#,
        )
        .unwrap_err();

        assert_eq!(
            refused,
            DeviceError::Refused("Device Flow has not been enabled for this app.".to_owned())
        );
    }

    /// THE TWO THAT ARE NOT FAILURES, and this is the whole reason polling is
    /// parsed rather than pattern-matched on a status code.
    ///
    /// GitHub answers this endpoint with HTTP 200 and an `error` field while
    /// the person is still typing their code. Treating either of these as an
    /// error ends an authorisation somebody is halfway through, and the person
    /// sees the connect dialog give up while GitHub is still waiting for them.
    #[test]
    fn waiting_is_not_failing() {
        assert_eq!(
            read_outcome(r#"{"error":"authorization_pending"}"#).unwrap(),
            DeviceOutcome::Pending
        );
        // And slow_down carries a NEW interval that is authoritative; ignoring
        // it earns another slow_down and eventually a ban.
        assert_eq!(
            read_outcome(r#"{"error":"slow_down","interval":10}"#).unwrap(),
            DeviceOutcome::SlowDown { interval: 10 }
        );
    }

    /// The three that really are over, told apart because a person who declined
    /// and a code that aged out need different words on screen.
    #[test]
    fn refusing_and_expiring_are_different_endings() {
        assert_eq!(
            read_outcome(r#"{"error":"access_denied"}"#).unwrap(),
            DeviceOutcome::Denied
        );
        assert_eq!(
            read_outcome(r#"{"error":"expired_token"}"#).unwrap(),
            DeviceOutcome::Expired
        );
        assert!(matches!(
            read_outcome(r#"{"error":"incorrect_client_credentials","error_description":"Bad id."}"#),
            Err(DeviceError::Refused(message)) if message == "Bad id."
        ));
    }

    #[test]
    fn a_granted_token_carries_its_refresh_and_its_expiry() {
        let outcome = read_outcome(
            r#"{"access_token":"ghu_live","refresh_token":"ghr_live",
                "expires_in":28800,"refresh_token_expires_in":15811200}"#,
        )
        .unwrap();

        let DeviceOutcome::Granted(tokens) = outcome else {
            panic!("expected a grant");
        };
        assert_eq!(tokens.access_token, "ghu_live");
        // The app expires user tokens, so a refresh token is the difference
        // between a connection that survives the day and one that does not.
        assert_eq!(tokens.refresh_token.as_deref(), Some("ghr_live"));
        assert_eq!(tokens.expires_in, Some(28_800));
    }

    /// Success without a token is not success.
    #[test]
    fn a_grant_with_no_token_is_refused_rather_than_stored() {
        assert!(matches!(
            read_outcome(r#"{"token_type":"bearer"}"#),
            Err(DeviceError::Refused(_))
        ));
    }

    #[test]
    fn a_connection_can_name_the_account_it_belongs_to() {
        assert_eq!(
            read_login(r#"{"login":"miopea","id":1}"#).unwrap(),
            "miopea"
        );
        // A connection that cannot name its account cannot be audited, so an
        // unreadable answer is a refusal rather than a blank name.
        assert!(matches!(
            read_login(r#"{"id":1}"#),
            Err(DeviceError::Refused(_))
        ));
        assert!(matches!(
            read_login(r#"{"login":""}"#),
            Err(DeviceError::Refused(_))
        ));
    }

    /// A refresh that GitHub declines is a connection that is over, not one to
    /// retry — and it must be told apart from a grant, or a dead connection
    /// looks alive forever.
    #[test]
    fn a_refused_refresh_is_a_connection_that_has_ended() {
        // The shape GitHub returns when a refresh token has been revoked.
        assert!(matches!(
            read_outcome(r#"{"error":"bad_refresh_token","error_description":"The refresh token passed is incorrect or expired."}"#),
            Err(DeviceError::Refused(message)) if message.contains("incorrect or expired")
        ));
        // And a good one still parses as a grant, with its successor tokens.
        let DeviceOutcome::Granted(tokens) = read_outcome(
            r#"{"access_token":"ghu_new","refresh_token":"ghr_new","expires_in":28800}"#,
        )
        .unwrap() else {
            panic!("expected a grant");
        };
        assert_eq!(tokens.access_token, "ghu_new");
        // The refresh token ROTATES. Keeping the old one is how a connection
        // dies at the next refresh instead of this one.
        assert_eq!(tokens.refresh_token.as_deref(), Some("ghr_new"));
    }
}

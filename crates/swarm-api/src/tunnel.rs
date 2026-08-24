//! A temporary public address for this Hive, for getting it onto a phone.
//!
//! Runs `cloudflared` as a quick tunnel: no Cloudflare account, no domain, and
//! a random `trycloudflare.com` hostname that lasts until the tunnel stops.
//!
//! Deliberately temporary, and deliberately not the way to publish a Hive for
//! good. The hostname changes every time, and three things in Swarm are bound
//! to an origin: passkeys are registered against a domain, an installed PWA is
//! a different app on a different origin, and the session cookie is per-origin.
//! A durable address wants a named tunnel on a hostname that holds still. This
//! exists for the first five minutes, and says so.
//!
//! Never started on its own. It publishes the control room to the internet, so
//! it starts when the operator asks and stops when they say.

use std::fmt::Write as _;
use std::net::SocketAddr;
use std::process::Stdio;
use std::sync::Arc;

use axum::{Json, extract::State, http::HeaderMap, http::StatusCode, response::IntoResponse};
use serde::Serialize;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

use crate::{ApiError, AppState, authorize, unix_timestamp};

/// How long to wait for cloudflared to announce a hostname before giving up.
const HOSTNAME_TIMEOUT_SECONDS: u64 = 30;
/// How long the address gets to start actually serving before we admit it is
/// not going to.
///
/// cloudflared prints the address and then says, in its own banner, "it may
/// take some time to be reachable". Measured 2026-08-24 on this host: the DNS
/// record appeared about three seconds after the address was printed, and the
/// edge still answered 404 with ZERO requests reaching cloudflared two minutes
/// later. So "printed" and "serving" are genuinely different states, and the
/// operator was shown a QR code for the first while believing it was the
/// second.
const REACHABLE_TIMEOUT_SECONDS: u64 = 45;

#[derive(Debug, Default)]
pub(super) struct TunnelSupervisor {
    inner: Arc<Mutex<Option<RunningTunnel>>>,
    failure: Arc<Mutex<LastFailure>>,
}

#[derive(Debug)]
struct RunningTunnel {
    child: Child,
    url: String,
    started_at: i64,
    /// Whether the address has been seen to serve yet.
    ///
    /// Verified in the background rather than inside the start request. It used
    /// to block for up to 45 seconds while holding this supervisor's lock, so
    /// every status poll queued behind it and the operator watched a spinner
    /// with no idea what was happening. Nothing that waits on a third party
    /// belongs inside a request the browser is holding open.
    serving: bool,
}

/// Why the last attempt to publish an address gave up, kept until the next one.
#[derive(Debug, Default)]
struct LastFailure(Option<String>);

#[derive(Debug, Serialize)]
pub(super) struct TunnelView {
    /// Whether `cloudflared` is installed at all.
    available: bool,
    running: bool,
    /// True once the address has actually answered. The QR is withheld until
    /// then, because an address that does not serve is a code to nowhere.
    serving: bool,
    /// Why the last attempt gave up, when it did.
    error: Option<String>,
    url: Option<String>,
    started_at: Option<i64>,
    /// A QR of the address, as an inline SVG. The address only — a token in a
    /// URL is a secret in browser history, in logs, and in every Referer.
    qr_svg: Option<String>,
}

fn cloudflared_available() -> bool {
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|directory| directory.join("cloudflared").is_file())
    })
}

/// Renders the address as an SVG QR code.
///
/// Built here rather than in the browser so the page needs no encoder and no
/// external script — the artifact CSP forbids one, and so does a control room
/// that must work with no internet beyond the tunnel it just opened.
/// Four, because ISO/IEC 18004 says four.
const QUIET_ZONE_MODULES: usize = 4;

fn qr_svg(url: &str) -> Option<String> {
    use qrcode::{EcLevel, QrCode};
    let code = QrCode::with_error_correction_level(url.as_bytes(), EcLevel::M).ok()?;
    let width = code.width();
    // ISO/IEC 18004 requires four modules of quiet zone. It was two, and
    // scanners rely on it to find the symbol at all — a QR with a thin border
    // reads as noise rather than as a code that failed to decode.
    let quiet = QUIET_ZONE_MODULES;
    let side = width + quiet * 2;
    let mut modules = String::new();
    for (index, dark) in code.into_colors().into_iter().enumerate() {
        if dark != qrcode::Color::Dark {
            continue;
        }
        let x = index % width + quiet;
        let y = index / width + quiet;
        let _ = write!(modules, r#"<rect x="{x}" y="{y}" width="1" height="1"/>"#);
    }
    // crispEdges is deliberately gone. It snaps every module edge to a whole
    // device pixel, so at any size that is not an exact multiple of the module
    // count the browser drops some rows and doubles others — which is what the
    // operator photographed. A slightly soft module scans; a missing one does
    // not.
    Some(format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {side} {side}" role="img" aria-label="QR code for this Hive's temporary address"><rect width="{side}" height="{side}" fill="#fff"/><g fill="#000">{modules}</g></svg>"##
    ))
}

impl TunnelSupervisor {
    async fn view(&self) -> TunnelView {
        let mut guard = self.inner.lock().await;
        // A tunnel whose process has exited is not running, whatever we last
        // recorded. Reported from the child rather than from our own state so a
        // cloudflared that died on its own does not leave a dead address on
        // screen for someone to scan.
        if guard
            .as_mut()
            .is_some_and(|tunnel| matches!(tunnel.child.try_wait(), Ok(Some(_)) | Err(_)))
        {
            *guard = None;
        }
        let failure = self.failure.lock().await.0.clone();
        match guard.as_ref() {
            Some(tunnel) => TunnelView {
                available: true,
                running: true,
                serving: tunnel.serving,
                error: None,
                url: Some(tunnel.url.clone()),
                started_at: Some(tunnel.started_at),
                // Withheld until the address answers. A QR for something that
                // does not serve is worse than no QR: it is scanned, it fails,
                // and the address looks like the thing that is broken.
                qr_svg: tunnel.serving.then(|| qr_svg(&tunnel.url)).flatten(),
            },
            None => TunnelView {
                available: cloudflared_available(),
                running: false,
                serving: false,
                error: failure,
                url: None,
                started_at: None,
                qr_svg: None,
            },
        }
    }

    async fn start(&self, address: SocketAddr) -> Result<TunnelView, ApiError> {
        let mut guard = self.inner.lock().await;
        if let Some(tunnel) = guard.as_mut() {
            if matches!(tunnel.child.try_wait(), Ok(None)) {
                let url = tunnel.url.clone();
                let started_at = tunnel.started_at;
                let serving = tunnel.serving;
                return Ok(TunnelView {
                    available: true,
                    running: true,
                    serving,
                    error: None,
                    qr_svg: serving.then(|| qr_svg(&url)).flatten(),
                    url: Some(url),
                    started_at: Some(started_at),
                });
            }
            *guard = None;
        }
        if !cloudflared_available() {
            return Err(ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "cloudflared_missing",
                "cloudflared is not installed on this machine",
            ));
        }
        let mut child = Command::new("cloudflared")
            .args([
                // An empty config, deliberately, and this is the whole reason
                // the feature never worked.
                //
                // `cloudflared tunnel --url ...` reads ~/.cloudflared/config.yml
                // if it exists. On a machine that already runs a NAMED tunnel —
                // which is the normal state for anyone who has published a Hive
                // properly — that file carries the named tunnel's credentials
                // and its ingress rules. cloudflared then prints a fresh quick
                // hostname while running the named tunnel's ingress, the new
                // hostname matches none of its rules, and every request falls
                // through to the catch-all. The operator's own config ends in
                // `- service: http_status:404`, which is where the 404 came
                // from. Measured 2026-08-24: identical origin, identical
                // command, 404 forever with the ambient config and 200 within
                // five seconds without it.
                "--config",
                "/dev/null",
                "tunnel",
                "--no-autoupdate",
                "--url",
                &format!("http://{address}"),
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| {
                ApiError::new(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "cloudflared_failed",
                    format!("cloudflared could not be started: {error}"),
                )
            })?;
        let Some(stderr) = child.stderr.take() else {
            let _ = child.kill().await;
            return Err(ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "cloudflared_failed",
                "cloudflared produced no output to read its address from",
            ));
        };
        // The reader outlives this function, and that is the whole fix.
        //
        // read_tunnel_hostname used to OWN stderr and return as soon as it
        // matched the address. Returning dropped the BufReader, which closed
        // the read end of the pipe; cloudflared kept logging, its next write
        // took SIGPIPE, and the process died seconds after we reported success.
        // The address was real and had already stopped being served — which is
        // exactly what Cloudflare error 1033 says.
        //
        // Draining for the life of the child costs one task and nothing else.
        let (found, url_rx) = tokio::sync::oneshot::channel();
        tokio::spawn(drain_tunnel_output(stderr, found));
        let Ok(Ok(url)) = tokio::time::timeout(
            std::time::Duration::from_secs(HOSTNAME_TIMEOUT_SECONDS),
            url_rx,
        )
        .await
        else {
            let _ = child.kill().await;
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                "cloudflared_no_address",
                "cloudflared did not report an address; check that this machine can reach Cloudflare",
            ));
        };
        let started_at = unix_timestamp();
        *guard = Some(RunningTunnel {
            child,
            url: url.clone(),
            started_at,
            serving: false,
        });
        self.failure.lock().await.0 = None;
        drop(guard);

        // Whether the address actually serves is settled in the background.
        //
        // It used to be settled inside this request: up to 45 seconds holding
        // the browser open AND this supervisor's lock, so every status poll
        // queued behind it. The operator got a spinner and then a bare status
        // code. Nothing that waits on a third party belongs inside a request
        // somebody is holding open.
        //
        // Until it answers, `running` is true and `serving` is false, and the
        // QR is withheld. A code for an address that does not serve is worse
        // than no code: it gets scanned, it fails, and the address looks like
        // the thing that is broken.
        self.verify_in_background(url.clone());

        Ok(TunnelView {
            available: true,
            running: true,
            serving: false,
            error: None,
            qr_svg: None,
            url: Some(url),
            started_at: Some(started_at),
        })
    }

    /// Settles whether the address serves, without holding anything open.
    ///
    /// Kills the tunnel and records the reason if it never does, so the next
    /// status read tells the operator what happened rather than leaving a card
    /// for an address that goes nowhere.
    fn verify_in_background(&self, url: String) {
        let inner = Arc::clone(&self.inner);
        let failure = Arc::clone(&self.failure);
        tokio::spawn(async move {
            let outcome = verify_serving(&url).await;
            let mut guard = inner.lock().await;
            // Only if it is still the same tunnel: the operator may have
            // stopped it and started another while this was waiting.
            if guard.as_ref().is_none_or(|tunnel| tunnel.url != url) {
                return;
            }
            match outcome {
                Ok(()) => {
                    if let Some(tunnel) = guard.as_mut() {
                        tunnel.serving = true;
                    }
                }
                Err(reason) => {
                    if let Some(mut tunnel) = guard.take() {
                        let _ = tunnel.child.kill().await;
                    }
                    failure.lock().await.0 = Some(reason);
                }
            }
        });
    }

    async fn stop(&self) -> TunnelView {
        let mut guard = self.inner.lock().await;
        if let Some(mut tunnel) = guard.take() {
            let _ = tunnel.child.kill().await;
        }
        self.failure.lock().await.0 = None;
        TunnelView {
            available: cloudflared_available(),
            running: false,
            serving: false,
            error: None,
            url: None,
            started_at: None,
            qr_svg: None,
        }
    }
}

/// Polls the address until it serves, or says why it never did.
///
/// Any HTTP answer counts, including one the Hive itself would refuse: what is
/// being established is that Cloudflare routes to this machine, not that a
/// particular route exists. A 404 from Cloudflare's own edge is NOT an answer —
/// that is what an unrouted quick tunnel returns, and telling the two apart is
/// the entire point of this check.
async fn verify_serving(url: &str) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .map_err(|error| format!("the address could not be checked: {error}"))?;
    let deadline =
        tokio::time::Instant::now() + std::time::Duration::from_secs(REACHABLE_TIMEOUT_SECONDS);
    let mut last = String::from("it never answered");
    while tokio::time::Instant::now() < deadline {
        match client.get(url).send().await {
            Ok(response) => {
                let status = response.status();
                let from_edge = response
                    .headers()
                    .get("server")
                    .and_then(|value| value.to_str().ok())
                    .is_some_and(|server| server.eq_ignore_ascii_case("cloudflare"));
                if !(status == reqwest::StatusCode::NOT_FOUND && from_edge) {
                    return Ok(());
                }
                last = format!(
                    "Cloudflare answered {status} for it without routing to this machine"
                );
            }
            Err(error) => {
                last = format!("it could not be reached: {error}");
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
    Err(format!(
        "The address was created but never started serving within {REACHABLE_TIMEOUT_SECONDS} seconds — {last}. Nothing was published; try again."
    ))
}

/// Reads cloudflared's banner for the hostname, then keeps reading.
///
/// The second half matters as much as the first: cloudflared logs for as long
/// as it runs, and a reader that stops reading closes the pipe under it. This
/// returns only at EOF, which is when the child has gone.
async fn drain_tunnel_output(
    stderr: tokio::process::ChildStderr,
    found: tokio::sync::oneshot::Sender<String>,
) {
    let mut lines = BufReader::new(stderr).lines();
    let mut found = Some(found);
    while let Ok(Some(line)) = lines.next_line().await {
        if let Some(url) = extract_quick_tunnel_url(&line)
            && let Some(sender) = found.take()
        {
            // The receiver is gone if the caller timed out. Keep draining
            // regardless: the child is still alive until someone kills it.
            let _ = sender.send(url);
        }
    }
}

/// Pulls the assigned address out of one line of cloudflared output.
///
/// Matched on the hostname suffix rather than on the shape of the banner, which
/// is decorated with box-drawing characters and has changed between releases.
fn extract_quick_tunnel_url(line: &str) -> Option<String> {
    let start = line.find("https://")?;
    let rest = &line[start + "https://".len()..];
    // Read exactly a hostname and stop. Reading to the next space instead let
    // the box-drawing that cloudflared pads its banner with become part of the
    // address, and an address with a `+` on the end is a QR code that goes
    // nowhere.
    let host: String = rest
        .chars()
        .take_while(|character| character.is_ascii_alphanumeric() || *character == '-' || *character == '.')
        .collect();
    host.strip_suffix(".trycloudflare.com")
        .is_some()
        .then(|| format!("https://{host}"))
}

pub(super) async fn status(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    authorize(&state, &headers)?;
    Ok(Json(state.tunnel.view().await))
}

pub(super) async fn start(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    authorize(&state, &headers)?;
    let address = state.api_bind_address.ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "bind_address_unknown",
            "this Hive does not know its own address, so it cannot be tunnelled",
        )
    })?;
    Ok(Json(state.tunnel.start(address).await?))
}

pub(super) async fn stop(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    authorize(&state, &headers)?;
    Ok(Json(state.tunnel.stop().await))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bug that shipped: reading the address killed the tunnel.
    ///
    /// The reader used to own stderr and return the moment it matched. That
    /// dropped the pipe, and cloudflared — which logs continuously — took
    /// SIGPIPE on its next write and died seconds after Swarm reported
    /// success. The operator scanned a real address that was already dead, and
    /// Cloudflare answered 1033.
    ///
    /// Reproduced with a process that behaves the same way: print the address,
    /// then keep writing. It survives only if something keeps reading.
    #[tokio::test]
    async fn a_process_that_keeps_logging_survives_us_reading_its_address() {
        let mut child = Command::new("sh")
            .args([
                "-c",
                // Announce, then log every 50ms for 10s. Any write to a closed
                // pipe kills this with SIGPIPE, which is exactly cloudflared's
                // behaviour and exactly what we are testing for.
                "echo 'INF |  https://kept-alive-test.trycloudflare.com  |' >&2; \
                 i=0; while [ $i -lt 200 ]; do echo \"INF still connected $i\" >&2; \
                 sleep 0.05; i=$((i+1)); done",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .expect("test child starts");
        let stderr = child.stderr.take().expect("piped stderr");

        let (found, url_rx) = tokio::sync::oneshot::channel();
        tokio::spawn(drain_tunnel_output(stderr, found));
        let url = tokio::time::timeout(std::time::Duration::from_secs(5), url_rx)
            .await
            .expect("the address arrives")
            .expect("the sender is not dropped");
        assert_eq!(url, "https://kept-alive-test.trycloudflare.com");

        // The address is in hand. Now let it write a great deal more than a
        // pipe buffer would hold, and confirm it is still alive — which is the
        // whole difference between a tunnel that serves and error 1033.
        tokio::time::sleep(std::time::Duration::from_millis(1_500)).await;
        assert!(
            matches!(child.try_wait(), Ok(None)),
            "the child must still be running after we have taken its address"
        );
        let _ = child.kill().await;
    }

    /// A scanner finds a symbol by its quiet zone. Two modules is not enough,
    /// and a photograph of the result reads as noise rather than as a code that
    /// failed to decode.
    #[test]
    fn the_qr_carries_the_quiet_zone_the_standard_requires() {
        let svg = qr_svg("https://neat-lion-quiet-fox.trycloudflare.com").expect("a code");
        let side: usize = svg
            .split("viewBox=\"0 0 ")
            .nth(1)
            .and_then(|rest| rest.split(' ').next())
            .and_then(|value| value.parse().ok())
            .expect("a square viewBox");

        let code = qrcode::QrCode::with_error_correction_level(
            "https://neat-lion-quiet-fox.trycloudflare.com".as_bytes(),
            qrcode::EcLevel::M,
        )
        .expect("a code");
        assert_eq!(
            side,
            code.width() + QUIET_ZONE_MODULES * 2,
            "the symbol must sit inside four modules of quiet zone on every side"
        );
        assert_eq!(QUIET_ZONE_MODULES, 4, "ISO/IEC 18004 requires four");

        // crispEdges snapped module edges to whole device pixels, so at a size
        // that was not an exact multiple of the module count the browser dropped
        // rows and doubled others. That is what the operator photographed.
        assert!(
            !svg.contains("crispEdges"),
            "a slightly soft module scans; a missing one does not"
        );
    }

    /// cloudflared draws its address inside a box, and the decoration has
    /// changed between releases. Matching the hostname rather than the banner
    /// is what keeps this working across an autoupdate.
    #[test]
    fn reads_the_address_out_of_cloudflared_banner_decoration() {
        assert_eq!(
            extract_quick_tunnel_url(
                "2026-08-23T12:00:00Z INF |  https://neat-lion-quiet-fox.trycloudflare.com   |"
            )
            .as_deref(),
            Some("https://neat-lion-quiet-fox.trycloudflare.com")
        );
        assert_eq!(
            extract_quick_tunnel_url("+---https://a-b-c-d.trycloudflare.com/---+").as_deref(),
            Some("https://a-b-c-d.trycloudflare.com")
        );
    }

    /// Only the quick-tunnel hostname counts. cloudflared logs plenty of other
    /// https URLs — its own documentation, the API it talks to — and treating
    /// one of those as the Hive's address would print a QR code that sends the
    /// operator's phone somewhere else entirely.
    #[test]
    fn ignores_every_other_address_cloudflared_prints() {
        for line in [
            "INF See https://developers.cloudflare.com/cloudflare-one/ for more",
            "INF Requesting new quick tunnel on trycloudflare.com...",
            "INF Connection registered connIndex=0 location=https://region1.v2.argotunnel.com",
            "no url here at all",
        ] {
            assert_eq!(extract_quick_tunnel_url(line), None, "{line}");
        }
    }

    #[test]
    fn renders_a_scannable_svg_for_the_address() {
        let svg = qr_svg("https://neat-lion-quiet-fox.trycloudflare.com").unwrap();
        assert!(svg.starts_with("<svg xmlns="));
        assert!(svg.contains("viewBox"));
        assert!(svg.contains("<rect"));
    }
}

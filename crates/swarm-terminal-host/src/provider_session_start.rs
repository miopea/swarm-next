//! Silent, bounded command-hook transport. This does not reconcile durable state.
use std::{
    env,
    io::{self, Read},
    os::fd::AsFd,
    path::PathBuf,
    time::{Duration, Instant},
};

use nix::poll::{PollFd, PollFlags, PollTimeout, poll};
use swarm_domain::WorkerSessionId;
use swarm_terminal::{
    HostClient, HostRequest, HostResponse, MAX_PROVIDER_LIFECYCLE_BYTES,
    ProviderLifecycleCapability, default_terminal_socket_path, read_claude_session_start,
};

const DEADLINE: Duration = Duration::from_secs(3);
const STARTUP_PROTOCOL: u16 = 15;

fn unavailable() -> io::Error {
    io::Error::other("provider startup evidence unavailable")
}

fn identity(
    session: &str,
    secret: &str,
) -> io::Result<(WorkerSessionId, ProviderLifecycleCapability)> {
    let session = session.parse().map_err(|_| unavailable())?;
    if secret.len() != 64 || !secret.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(unavailable());
    }
    let mut capability = [0; 32];
    for (index, byte) in capability.iter_mut().enumerate() {
        *byte =
            u8::from_str_radix(&secret[index * 2..index * 2 + 2], 16).map_err(|_| unavailable())?;
    }
    Ok((session, ProviderLifecycleCapability(capability)))
}

// Do not use Tokio stdin: its blocking read cannot be canceled on timeout and
// can keep runtime shutdown waiting forever when the provider holds stdin open.
// This helper exclusively owns stdin. Poll bounds each read without changing
// shared file-description flags inherited from the provider.
fn read_input(input: &mut (impl Read + AsFd), deadline: Instant) -> io::Result<Vec<u8>> {
    let mut result = Vec::new();
    loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(unavailable)?;
        let timeout = PollTimeout::try_from(remaining).map_err(|_| unavailable())?;
        let mut descriptors = [PollFd::new(input.as_fd(), PollFlags::POLLIN)];
        let ready = poll(&mut descriptors, timeout).map_err(|_| unavailable())?;
        if ready == 0 {
            return Err(unavailable());
        }
        let flags = descriptors[0].revents().ok_or_else(unavailable)?;
        if flags.intersects(PollFlags::POLLERR | PollFlags::POLLNVAL) {
            return Err(unavailable());
        }
        let mut buffer = [0; 4096];
        let limit = buffer
            .len()
            .min(MAX_PROVIDER_LIFECYCLE_BYTES + 1 - result.len());
        let count = input
            .read(&mut buffer[..limit])
            .map_err(|_| unavailable())?;
        if count == 0 {
            return Ok(result);
        }
        result.extend_from_slice(&buffer[..count]);
        if result.len() > MAX_PROVIDER_LIFECYCLE_BYTES {
            return Err(unavailable());
        }
    }
}

pub async fn run(resume_end: bool) -> io::Result<()> {
    // SessionEnd has a provider-owned 1.5-second budget. Finish within one
    // second rather than extending that budget or keeping shutdown waiting.
    let deadline = Instant::now()
        + if resume_end {
            Duration::from_secs(1)
        } else {
            DEADLINE
        };
    let (session_id, capability) = identity(
        &env::var("SWARM_PROVIDER_SESSION").map_err(|_| unavailable())?,
        &env::var("SWARM_PROVIDER_START_CAPABILITY").map_err(|_| unavailable())?,
    )?;
    // Read the descriptor directly: StdinLock's internal buffering could consume
    // bytes which a subsequent descriptor poll would no longer see.
    let descriptor = io::stdin()
        .as_fd()
        .try_clone_to_owned()
        .map_err(|_| unavailable())?;
    let input = read_input(&mut std::fs::File::from(descriptor), deadline)?;
    let socket = env::var_os("SWARM_TERMINAL_SOCKET")
        .map_or_else(default_terminal_socket_path, PathBuf::from);
    let client = HostClient::new(socket);
    let request = if resume_end {
        HostRequest::ProviderResumeEnd {
            session_id,
            capability,
            previous: swarm_terminal::read_claude_resume_end(&input).ok_or_else(unavailable)?,
        }
    } else {
        HostRequest::ProviderSessionStart {
            session_id,
            capability,
            observation: read_claude_session_start(&input).ok_or_else(unavailable)?,
        }
    };
    send_until(&client, &request, deadline).await
}

async fn send_until(
    client: &HostClient,
    request: &HostRequest,
    deadline: Instant,
) -> io::Result<()> {
    tokio::time::timeout_at(deadline.into(), send(client, request))
        .await
        .map_err(|_| unavailable())?
}

async fn send(client: &HostClient, request: &HostRequest) -> io::Result<()> {
    // Unknown newer protocols are not presumed compatible. Never send secrets
    // in a speculative request to an older host during a rolling update.
    if !matches!(client.request(&HostRequest::Ping).await,
        Ok(HostResponse::Pong { protocol_version }) if protocol_version == STARTUP_PROTOCOL)
    {
        return Err(unavailable());
    }
    match client.request(request).await {
        Ok(HostResponse::Acknowledged) => Ok(()),
        _ => Err(unavailable()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs::File,
        io::{Seek, SeekFrom, Write},
    };

    #[test]
    fn identity_rejects_malformed_secrets_without_echoing_them() {
        let session = WorkerSessionId::new().to_string();
        assert!(identity(&session, &"ab".repeat(32)).is_ok());
        for secret in ["private-token".to_owned(), "é".repeat(32), "gg".repeat(32)] {
            let error = identity(&session, &secret).unwrap_err().to_string();
            assert!(!error.contains(&secret));
        }
        assert!(identity("not-a-session", &"ab".repeat(32)).is_err());
    }

    #[test]
    fn input_has_size_and_open_pipe_deadlines() {
        let mut file = tempfile::tempfile().unwrap();
        file.write_all(&vec![b'x'; MAX_PROVIDER_LIFECYCLE_BYTES + 1])
            .unwrap();
        file.seek(SeekFrom::Start(0)).unwrap();
        assert!(read_input(&mut file, Instant::now() + DEADLINE).is_err());
        file.set_len(MAX_PROVIDER_LIFECYCLE_BYTES as u64).unwrap();
        file.seek(SeekFrom::Start(0)).unwrap();
        assert_eq!(
            read_input(&mut file, Instant::now() + DEADLINE)
                .unwrap()
                .len(),
            MAX_PROVIDER_LIFECYCLE_BYTES
        );

        let (reader, _held_writer) = nix::unistd::pipe().unwrap();
        let mut reader = File::from(reader);
        assert!(read_input(&mut reader, Instant::now() + Duration::from_millis(10)).is_err());
    }

    #[tokio::test]
    async fn compatible_engine_receives_observation_and_acknowledges() {
        use tokio::{
            io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
            net::UnixListener,
        };
        assert_eq!(STARTUP_PROTOCOL, swarm_terminal::PROTOCOL_VERSION);
        let directory = tempfile::tempdir().unwrap();
        let socket = directory.path().join("startup.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let session_id = WorkerSessionId::new();
        let request = HostRequest::ProviderSessionStart {
            session_id,
            capability: ProviderLifecycleCapability([173; 32]),
            observation: swarm_terminal::ProviderSessionStartObservation {
                conversation: swarm_domain::ProviderConversationId::new(),
                kind: swarm_domain::ProviderSessionStartKind::Resumed,
            },
        };
        let server = async {
            for response in [
                HostResponse::Pong {
                    protocol_version: STARTUP_PROTOCOL,
                },
                HostResponse::Acknowledged,
            ] {
                let (stream, _) = listener.accept().await.unwrap();
                let mut reader = BufReader::new(stream);
                let mut line = String::new();
                reader.read_line(&mut line).await.unwrap();
                let received: HostRequest = serde_json::from_str(&line).unwrap();
                match &response {
                    HostResponse::Pong { .. } => assert!(matches!(received, HostRequest::Ping)),
                    _ => assert_eq!(
                        serde_json::to_value(received).unwrap(),
                        serde_json::to_value(&request).unwrap()
                    ),
                }
                let mut bytes = serde_json::to_vec(&response).unwrap();
                bytes.push(b'\n');
                reader.get_mut().write_all(&bytes).await.unwrap();
            }
        };
        let client = HostClient::new(socket);
        let ((), result) = tokio::time::timeout(DEADLINE, async {
            tokio::join!(
                server,
                send_until(&client, &request, Instant::now() + DEADLINE)
            )
        })
        .await
        .unwrap();
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn incompatible_engine_receives_only_ping() {
        use tokio::{
            io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
            net::UnixListener,
        };
        for protocol_version in [11, 12, 13, 15] {
            let directory = tempfile::tempdir().unwrap();
            let socket = directory.path().join("engine.sock");
            let listener = UnixListener::bind(&socket).unwrap();
            let server = async {
                let (stream, _) = listener.accept().await.unwrap();
                let mut reader = BufReader::new(stream);
                let mut line = String::new();
                reader.read_line(&mut line).await.unwrap();
                assert!(matches!(
                    serde_json::from_str::<HostRequest>(&line).unwrap(),
                    HostRequest::Ping
                ));
                let mut pong =
                    serde_json::to_vec(&HostResponse::Pong { protocol_version }).unwrap();
                pong.push(b'\n');
                reader.get_mut().write_all(&pong).await.unwrap();
            };
            // A sentinel request must never be sent after a failed preflight.
            let client = HostClient::new(socket);
            let ((), result) = tokio::time::timeout(DEADLINE, async {
                tokio::join!(
                    server,
                    send_until(&client, &HostRequest::Ping, Instant::now() + DEADLINE)
                )
            })
            .await
            .unwrap();
            assert!(result.is_err());
            assert!(
                tokio::time::timeout(Duration::from_millis(10), listener.accept())
                    .await
                    .is_err()
            );
        }
    }

    #[tokio::test]
    async fn unresponsive_engine_does_not_hold_the_helper() {
        let directory = tempfile::tempdir().unwrap();
        let socket = directory.path().join("silent.sock");
        let listener = tokio::net::UnixListener::bind(&socket).unwrap();
        let client = HostClient::new(socket);
        let server = async {
            let (stream, _) = listener.accept().await.unwrap();
            // Retain the accepted stream while the client reaches its deadline.
            stream
        };
        let (_, result) = tokio::time::timeout(DEADLINE, async {
            tokio::join!(
                server,
                send_until(
                    &client,
                    &HostRequest::Ping,
                    Instant::now() + Duration::from_millis(20)
                )
            )
        })
        .await
        .unwrap();
        assert!(result.is_err());
    }
}

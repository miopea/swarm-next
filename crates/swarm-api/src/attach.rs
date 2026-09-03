use std::{
    collections::HashMap,
    sync::{Mutex, MutexGuard},
    time::{Duration, Instant},
};

use swarm_domain::WorkerSessionId;
use thiserror::Error;

pub const ATTACH_GRANT_TTL: Duration = Duration::from_secs(30);
pub const MAX_ATTACH_GRANTS: usize = 128;

/// A one-time grant cannot be reused to negotiate a weaker input contract.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AttachProtocol {
    #[default]
    Legacy,
    Controlled,
}

#[derive(Debug, Error)]
pub enum AttachGrantError {
    #[error("attach grant capacity of {limit} reached")]
    CapacityReached { limit: usize },
    #[error("secure attach grant generation failed")]
    RandomnessUnavailable,
    #[error("attach grant store lock was poisoned")]
    LockPoisoned,
}

#[derive(Clone, Copy, Debug)]
struct Grant {
    session_id: WorkerSessionId,
    protocol: AttachProtocol,
    expires_at: Instant,
}

#[derive(Debug)]
pub struct AttachGrantStore {
    grants: Mutex<HashMap<String, Grant>>,
    max_grants: usize,
    ttl: Duration,
}

impl Default for AttachGrantStore {
    fn default() -> Self {
        Self::new(MAX_ATTACH_GRANTS, ATTACH_GRANT_TTL)
    }
}

impl AttachGrantStore {
    #[must_use]
    pub fn new(max_grants: usize, ttl: Duration) -> Self {
        Self {
            grants: Mutex::new(HashMap::new()),
            max_grants,
            ttl,
        }
    }

    #[cfg(test)]
    pub fn issue(&self, session_id: WorkerSessionId) -> Result<String, AttachGrantError> {
        self.issue_for(session_id, AttachProtocol::Legacy)
    }

    #[cfg(test)]
    pub fn consume(&self, token: &str, session_id: WorkerSessionId) -> bool {
        self.consume_for(token, session_id, AttachProtocol::Legacy)
    }

    pub fn issue_for(
        &self,
        session_id: WorkerSessionId,
        protocol: AttachProtocol,
    ) -> Result<String, AttachGrantError> {
        self.issue_at(session_id, protocol, Instant::now())
    }

    pub fn consume_for(
        &self,
        token: &str,
        session_id: WorkerSessionId,
        protocol: AttachProtocol,
    ) -> bool {
        self.consume_at(token, session_id, protocol, Instant::now())
            .unwrap_or(false)
    }

    fn issue_at(
        &self,
        session_id: WorkerSessionId,
        protocol: AttachProtocol,
        now: Instant,
    ) -> Result<String, AttachGrantError> {
        let mut grants = self.lock()?;
        grants.retain(|_, grant| grant.expires_at > now);
        if grants.len() >= self.max_grants {
            return Err(AttachGrantError::CapacityReached {
                limit: self.max_grants,
            });
        }
        for _ in 0..4 {
            let token = random_token()?;
            if let std::collections::hash_map::Entry::Vacant(entry) = grants.entry(token.clone()) {
                entry.insert(Grant {
                    session_id,
                    protocol,
                    expires_at: now + self.ttl,
                });
                return Ok(token);
            }
        }
        Err(AttachGrantError::RandomnessUnavailable)
    }

    fn consume_at(
        &self,
        token: &str,
        session_id: WorkerSessionId,
        protocol: AttachProtocol,
        now: Instant,
    ) -> Result<bool, AttachGrantError> {
        let mut grants = self.lock()?;
        let Some(grant) = grants.remove(token) else {
            return Ok(false);
        };
        Ok(grant.expires_at > now && grant.session_id == session_id && grant.protocol == protocol)
    }

    fn lock(&self) -> Result<MutexGuard<'_, HashMap<String, Grant>>, AttachGrantError> {
        self.grants
            .lock()
            .map_err(|_| AttachGrantError::LockPoisoned)
    }
}

fn random_token() -> Result<String, AttachGrantError> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes).map_err(|_| AttachGrantError::RandomnessUnavailable)?;
    let mut token = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write;
        write!(&mut token, "{byte:02x}").expect("writing to a String cannot fail");
    }
    Ok(token)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grants_are_one_time_and_session_scoped() {
        let store = AttachGrantStore::default();
        let expected = WorkerSessionId::new();
        let other = WorkerSessionId::new();
        let wrong_session_token = store.issue(expected).unwrap();
        assert!(!store.consume(&wrong_session_token, other));
        assert!(!store.consume(&wrong_session_token, expected));

        let valid_token = store.issue(expected).unwrap();
        assert!(store.consume(&valid_token, expected));
        assert!(!store.consume(&valid_token, expected));
    }

    #[test]
    fn expired_grants_do_not_consume_capacity() {
        let store = AttachGrantStore::new(1, Duration::from_secs(1));
        let session_id = WorkerSessionId::new();
        let now = Instant::now();
        let expired = store
            .issue_at(session_id, AttachProtocol::Legacy, now)
            .unwrap();
        assert!(
            !store
                .consume_at(
                    &expired,
                    session_id,
                    AttachProtocol::Legacy,
                    now + Duration::from_secs(2)
                )
                .unwrap()
        );
        assert!(
            store
                .issue_at(
                    session_id,
                    AttachProtocol::Legacy,
                    now + Duration::from_secs(2)
                )
                .is_ok()
        );
    }

    #[test]
    fn controlled_grants_cannot_downgrade_and_failed_use_consumes_them() {
        let store = AttachGrantStore::default();
        let session = WorkerSessionId::new();
        let token = store
            .issue_for(session, AttachProtocol::Controlled)
            .unwrap();
        assert!(!store.consume(&token, session));
        assert!(!store.consume_for(&token, session, AttachProtocol::Controlled));
        let token = store
            .issue_for(session, AttachProtocol::Controlled)
            .unwrap();
        assert!(store.consume_for(&token, session, AttachProtocol::Controlled));
        assert!(!store.consume_for(&token, session, AttachProtocol::Controlled));
        let token = store.issue(session).unwrap();
        assert!(!store.consume_for(&token, session, AttachProtocol::Controlled));
    }
}

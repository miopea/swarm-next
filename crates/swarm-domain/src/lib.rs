use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WorkerId(Uuid);

impl WorkerId {
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl Default for WorkerId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WorkerSessionId(Uuid);

impl WorkerSessionId {
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl Default for WorkerSessionId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    ClaudeCode,
    Codex,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerSessionState {
    Starting,
    Running,
    Exited,
    Stopped,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WorkerSession {
    pub id: WorkerSessionId,
    pub worker_id: WorkerId,
    pub provider: ProviderKind,
    pub state: WorkerSessionState,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_identity_is_not_worker_identity() {
        let worker_id = WorkerId::new();
        let first = WorkerSession {
            id: WorkerSessionId::new(),
            worker_id,
            provider: ProviderKind::ClaudeCode,
            state: WorkerSessionState::Running,
        };
        let second = WorkerSession {
            id: WorkerSessionId::new(),
            worker_id,
            provider: ProviderKind::ClaudeCode,
            state: WorkerSessionState::Starting,
        };
        assert_ne!(first.id, second.id);
        assert_eq!(first.worker_id, second.worker_id);
    }
}

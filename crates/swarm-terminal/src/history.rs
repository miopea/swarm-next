use std::{
    collections::{HashMap, VecDeque},
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    str::FromStr,
    sync::{Mutex, MutexGuard},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use swarm_domain::WorkerSessionId;
use thiserror::Error;

use crate::TerminalSnapshot;

const RECORD_MAGIC: &[u8; 4] = b"SWH1";
const RECORD_VERSION: u8 = 1;
const OUTPUT_RECORD: u8 = 1;
const CHECKPOINT_RECORD: u8 = 2;
const RECORD_HEADER_BYTES: usize = 32;
const SYNC_INTERVAL_BYTES: u64 = 256 * 1024;
pub const MAX_HISTORY_RECORD_BYTES: u64 = 2 * 1024 * 1024 + 64;
pub const MAX_HISTORY_PAGE_BYTES: u64 = 512 * 1024;
pub const MAX_HISTORY_PAGE_RECORDS: usize = 2_048;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct HistoryLimits {
    pub max_record_bytes: u64,
    pub max_segment_bytes: u64,
    pub max_session_bytes: u64,
    pub max_total_bytes: u64,
    pub max_age_seconds: u64,
}

impl HistoryLimits {
    #[must_use]
    pub const fn new(
        max_record_bytes: u64,
        max_segment_bytes: u64,
        max_session_bytes: u64,
        max_total_bytes: u64,
        max_age_seconds: u64,
    ) -> Self {
        Self {
            max_record_bytes,
            max_segment_bytes,
            max_session_bytes,
            max_total_bytes,
            max_age_seconds,
        }
    }

    fn validate(self) -> Result<(), HistoryError> {
        let minimum_segment = self
            .max_record_bytes
            .checked_add(RECORD_HEADER_BYTES as u64)
            .ok_or(HistoryError::InvalidLimits)?;
        if self.max_record_bytes == 0
            || self.max_record_bytes > MAX_HISTORY_RECORD_BYTES
            || self.max_segment_bytes < minimum_segment
            || self.max_session_bytes < self.max_segment_bytes
            || self.max_total_bytes < self.max_session_bytes
            || self.max_age_seconds == 0
        {
            return Err(HistoryError::InvalidLimits);
        }
        Ok(())
    }
}

impl Default for HistoryLimits {
    fn default() -> Self {
        Self::new(
            MAX_HISTORY_RECORD_BYTES,
            4 * 1024 * 1024,
            64 * 1024 * 1024,
            512 * 1024 * 1024,
            7 * 24 * 60 * 60,
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct HistoryDiagnostics {
    pub limits: HistoryLimits,
    pub retained_bytes: u64,
    pub session_count: usize,
    pub segment_count: usize,
    pub dropped_records: u64,
    pub dropped_bytes: u64,
    pub recovered_truncated_bytes: u64,
    pub recovered_corrupt_segments: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HistoryAppendOutcome {
    Persisted,
    CheckpointRequired,
    DroppedAtCapacity,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct HistoryCursor {
    pub segment: u64,
    pub record: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct HistoryPage {
    pub session_id: WorkerSessionId,
    pub records: Vec<HistoryRecord>,
    pub next_cursor: Option<HistoryCursor>,
    pub has_more: bool,
    pub reset: bool,
    pub active: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct HistorySessionSummary {
    pub session_id: WorkerSessionId,
    pub retained_bytes: u64,
    pub active: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HistoryRecord {
    Output {
        sequence: u64,
        timestamp_millis: u64,
        bytes: Vec<u8>,
    },
    Checkpoint {
        timestamp_millis: u64,
        snapshot: TerminalSnapshot,
    },
}

impl HistoryRecord {
    #[must_use]
    pub const fn sequence(&self) -> u64 {
        match self {
            Self::Output { sequence, .. } => *sequence,
            Self::Checkpoint { snapshot, .. } => snapshot.sequence,
        }
    }

    const fn payload_bytes(&self) -> usize {
        match self {
            Self::Output { bytes, .. } => bytes.len(),
            Self::Checkpoint { snapshot, .. } => 5 + snapshot.bytes.len(),
        }
    }
}

#[derive(Debug, Error)]
pub enum HistoryError {
    #[error("terminal history limits are invalid")]
    InvalidLimits,
    #[error("terminal history directory is not a secure owned directory: {0}")]
    InsecureDirectory(PathBuf),
    #[error("terminal history session already exists")]
    SessionAlreadyExists,
    #[error("terminal history session was not found")]
    SessionNotFound,
    #[error("terminal history record exceeded the {limit}-byte limit")]
    RecordTooLarge { limit: u64 },
    #[error("terminal history lock was poisoned")]
    LockPoisoned,
    #[error("terminal history I/O failed: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug)]
struct Segment {
    index: u64,
    path: PathBuf,
    bytes: u64,
    modified: SystemTime,
}

#[derive(Debug)]
struct CurrentSegment {
    index: u64,
    file: File,
    unsynced_bytes: u64,
}

#[derive(Debug, Default)]
struct SessionHistory {
    segments: VecDeque<Segment>,
    current: Option<CurrentSegment>,
    next_segment: u64,
    active: bool,
}

impl SessionHistory {
    fn retained_bytes(&self) -> u64 {
        self.segments.iter().map(|segment| segment.bytes).sum()
    }
}

#[derive(Debug, Default)]
struct StoreState {
    sessions: HashMap<WorkerSessionId, SessionHistory>,
    retained_bytes: u64,
    dropped_records: u64,
    dropped_bytes: u64,
    recovered_truncated_bytes: u64,
    recovered_corrupt_segments: u64,
}

#[derive(Debug)]
pub struct HistoryStore {
    root: PathBuf,
    limits: HistoryLimits,
    state: Mutex<StoreState>,
}

impl HistoryStore {
    /// Opens a bounded terminal-history store and validates every retained
    /// record. An incomplete or corrupt segment tail is truncated to its last
    /// trustworthy record.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid limits, an insecure directory, I/O, or a
    /// poisoned store lock.
    pub fn open(root: impl Into<PathBuf>, limits: HistoryLimits) -> Result<Self, HistoryError> {
        limits.validate()?;
        let root = root.into();
        secure_directory(&root)?;
        let mut state = scan_store(&root, limits)?;
        prune_expired(&mut state, limits, SystemTime::now())?;
        prune_total(&mut state, limits, 0)?;
        cleanup_empty_sessions(&mut state, &root)?;
        Ok(Self {
            root,
            limits,
            state: Mutex::new(state),
        })
    }

    /// Registers a fresh immutable session identity.
    ///
    /// # Errors
    ///
    /// Returns an error for an identity collision, insecure path, I/O, or a
    /// poisoned store lock.
    pub fn start_session(&self, id: WorkerSessionId) -> Result<(), HistoryError> {
        let mut state = self.lock()?;
        if state.sessions.contains_key(&id) {
            return Err(HistoryError::SessionAlreadyExists);
        }
        let path = self.session_path(id);
        if fs::symlink_metadata(&path).is_ok() {
            return Err(HistoryError::SessionAlreadyExists);
        }
        secure_directory(&path)?;
        state.sessions.insert(
            id,
            SessionHistory {
                active: true,
                ..SessionHistory::default()
            },
        );
        Ok(())
    }

    /// Appends output at the exact sequence assigned by canonical terminal
    /// state. When the current segment cannot hold the record, the caller must
    /// supply a canonical checkpoint instead so the next segment is
    /// independently replayable.
    ///
    /// # Errors
    ///
    /// Returns an error for an unknown session, oversized record, I/O, or a
    /// poisoned store lock.
    pub fn append(
        &self,
        id: WorkerSessionId,
        sequence: u64,
        bytes: &[u8],
    ) -> Result<HistoryAppendOutcome, HistoryError> {
        self.append_at(id, sequence, bytes, SystemTime::now())
    }

    fn append_at(
        &self,
        id: WorkerSessionId,
        sequence: u64,
        bytes: &[u8],
        now: SystemTime,
    ) -> Result<HistoryAppendOutcome, HistoryError> {
        if bytes.len() as u64 > self.limits.max_record_bytes {
            return Err(HistoryError::RecordTooLarge {
                limit: self.limits.max_record_bytes,
            });
        }
        if bytes.is_empty() {
            return Ok(HistoryAppendOutcome::Persisted);
        }
        let record = encode_record(OUTPUT_RECORD, sequence, now, bytes);
        let record_bytes = record.len() as u64;
        let mut state = self.lock()?;
        let session = state
            .sessions
            .get(&id)
            .ok_or(HistoryError::SessionNotFound)?;
        let Some(current) = &session.current else {
            return Ok(HistoryAppendOutcome::CheckpointRequired);
        };
        let current_bytes = session
            .segments
            .iter()
            .find(|segment| segment.index == current.index)
            .map_or(0, |segment| segment.bytes);
        if current_bytes.saturating_add(record_bytes) > self.limits.max_segment_bytes {
            return Ok(HistoryAppendOutcome::CheckpointRequired);
        }
        append_record_locked(
            &mut state,
            &self.root,
            id,
            self.limits,
            &record,
            bytes.len() as u64,
            now,
        )
    }

    /// Writes a canonical checkpoint. Checkpoints begin every segment and may
    /// also be appended after a resize.
    ///
    /// # Errors
    ///
    /// Returns an error for an unknown session, oversized checkpoint, I/O, or
    /// a poisoned store lock.
    pub fn append_checkpoint(
        &self,
        id: WorkerSessionId,
        snapshot: &TerminalSnapshot,
    ) -> Result<HistoryAppendOutcome, HistoryError> {
        self.append_checkpoint_at(id, snapshot, SystemTime::now())
    }

    fn append_checkpoint_at(
        &self,
        id: WorkerSessionId,
        snapshot: &TerminalSnapshot,
        now: SystemTime,
    ) -> Result<HistoryAppendOutcome, HistoryError> {
        let payload = encode_checkpoint(snapshot);
        if payload.len() as u64 > self.limits.max_record_bytes {
            return Err(HistoryError::RecordTooLarge {
                limit: self.limits.max_record_bytes,
            });
        }
        let record = encode_record(CHECKPOINT_RECORD, snapshot.sequence, now, &payload);
        let mut state = self.lock()?;
        if !state.sessions.contains_key(&id) {
            return Err(HistoryError::SessionNotFound);
        }
        close_current(&mut state, id)?;
        append_record_locked(
            &mut state,
            &self.root,
            id,
            self.limits,
            &record,
            snapshot.bytes.len() as u64,
            now,
        )
    }

    /// Flushes the current segment and marks it eligible for global pruning.
    ///
    /// # Errors
    ///
    /// Returns an error for an unknown session, I/O, or a poisoned store lock.
    pub fn finish_session(&self, id: WorkerSessionId) -> Result<(), HistoryError> {
        let mut state = self.lock()?;
        let session = state
            .sessions
            .get_mut(&id)
            .ok_or(HistoryError::SessionNotFound)?;
        if let Some(current) = session.current.take() {
            current.file.sync_data()?;
        }
        session.active = false;
        cleanup_empty_sessions(&mut state, &self.root)?;
        Ok(())
    }

    /// Reads validated records for one session. The returned allocation is
    /// bounded by the configured per-session byte limit.
    ///
    /// # Errors
    ///
    /// Returns an error for an unknown session, I/O, or a poisoned store lock.
    pub fn read_session(&self, id: WorkerSessionId) -> Result<Vec<HistoryRecord>, HistoryError> {
        let mut files = {
            let state = self.lock()?;
            let session = state
                .sessions
                .get(&id)
                .ok_or(HistoryError::SessionNotFound)?;
            session
                .segments
                .iter()
                .map(|segment| File::open(&segment.path))
                .collect::<Result<Vec<_>, _>>()?
        };
        let mut records = Vec::new();
        for file in &mut files {
            records.extend(read_records(file, self.limits.max_record_bytes)?);
        }
        Ok(records)
    }

    /// Lists durable sessions without exposing terminal content.
    ///
    /// # Errors
    ///
    /// Returns an error if the store lock is poisoned.
    pub fn sessions(&self) -> Result<Vec<HistorySessionSummary>, HistoryError> {
        let state = self.lock()?;
        let mut sessions = state
            .sessions
            .iter()
            .map(|(session_id, session)| HistorySessionSummary {
                session_id: *session_id,
                retained_bytes: session.retained_bytes(),
                active: session.active,
            })
            .collect::<Vec<_>>();
        sessions.sort_by_key(|session| session.session_id.to_string());
        Ok(sessions)
    }

    /// Reads one bounded page of durable history. A cursor whose segment was
    /// evicted resets atomically to the oldest retained checkpoint.
    ///
    /// # Errors
    ///
    /// Returns an error for an unknown session, I/O, or a poisoned store lock.
    pub fn page(
        &self,
        id: WorkerSessionId,
        cursor: Option<HistoryCursor>,
    ) -> Result<HistoryPage, HistoryError> {
        self.page_with_bounds(id, cursor, MAX_HISTORY_PAGE_BYTES, MAX_HISTORY_PAGE_RECORDS)
    }

    fn page_with_bounds(
        &self,
        id: WorkerSessionId,
        cursor: Option<HistoryCursor>,
        max_page_bytes: u64,
        max_page_records: usize,
    ) -> Result<HistoryPage, HistoryError> {
        let (active, mut segments) = self.open_session_segments(id)?;
        if segments.is_empty() {
            return Ok(HistoryPage {
                session_id: id,
                records: Vec::new(),
                next_cursor: cursor,
                has_more: false,
                reset: false,
                active,
            });
        }

        let PageStart::Position {
            mut segment,
            mut record,
            mut reset,
        } = page_start(&segments, cursor.as_ref())
        else {
            return Ok(HistoryPage {
                session_id: id,
                records: Vec::new(),
                next_cursor: cursor,
                has_more: false,
                reset: false,
                active,
            });
        };

        let mut page_records = Vec::new();
        let mut page_bytes = 0_u64;
        let mut next_cursor = cursor;
        let mut has_more = false;
        while segment < segments.len() {
            let (segment_index, file) = &mut segments[segment];
            let records = read_records(file, self.limits.max_record_bytes)?;
            if record > records.len() {
                reset = true;
                segment = 0;
                record = 0;
                page_records.clear();
                page_bytes = 0;
                next_cursor = None;
                for (_, file) in &mut segments {
                    file.seek(SeekFrom::Start(0))?;
                }
                continue;
            }
            if record == records.len() {
                segment += 1;
                record = 0;
                continue;
            }
            for (index, history_record) in records.into_iter().enumerate().skip(record) {
                let record_bytes = history_record.payload_bytes() as u64;
                if !page_records.is_empty()
                    && (page_bytes.saturating_add(record_bytes) > max_page_bytes
                        || page_records.len() >= max_page_records)
                {
                    has_more = true;
                    next_cursor = Some(HistoryCursor {
                        segment: *segment_index,
                        record: u32::try_from(index).unwrap_or(u32::MAX),
                    });
                    break;
                }
                page_bytes = page_bytes.saturating_add(record_bytes);
                page_records.push(history_record);
                next_cursor = Some(HistoryCursor {
                    segment: *segment_index,
                    record: u32::try_from(index.saturating_add(1)).unwrap_or(u32::MAX),
                });
            }
            if has_more {
                break;
            }
            segment += 1;
            record = 0;
            if let Some((next_segment, _)) = segments.get(segment) {
                next_cursor = Some(HistoryCursor {
                    segment: *next_segment,
                    record: 0,
                });
            }
        }

        Ok(HistoryPage {
            session_id: id,
            records: page_records,
            next_cursor,
            has_more,
            reset,
            active,
        })
    }

    fn open_session_segments(
        &self,
        id: WorkerSessionId,
    ) -> Result<(bool, Vec<(u64, File)>), HistoryError> {
        let state = self.lock()?;
        let session = state
            .sessions
            .get(&id)
            .ok_or(HistoryError::SessionNotFound)?;
        let segments = session
            .segments
            .iter()
            .map(|segment| Ok((segment.index, File::open(&segment.path)?)))
            .collect::<Result<Vec<_>, std::io::Error>>()?;
        Ok((session.active, segments))
    }

    /// Returns content-free capacity and recovery diagnostics.
    ///
    /// # Errors
    ///
    /// Returns an error if the store lock is poisoned.
    pub fn diagnostics(&self) -> Result<HistoryDiagnostics, HistoryError> {
        let state = self.lock()?;
        Ok(HistoryDiagnostics {
            limits: self.limits,
            retained_bytes: state.retained_bytes,
            session_count: state.sessions.len(),
            segment_count: state
                .sessions
                .values()
                .map(|session| session.segments.len())
                .sum(),
            dropped_records: state.dropped_records,
            dropped_bytes: state.dropped_bytes,
            recovered_truncated_bytes: state.recovered_truncated_bytes,
            recovered_corrupt_segments: state.recovered_corrupt_segments,
        })
    }

    fn lock(&self) -> Result<MutexGuard<'_, StoreState>, HistoryError> {
        self.state.lock().map_err(|_| HistoryError::LockPoisoned)
    }

    fn session_path(&self, id: WorkerSessionId) -> PathBuf {
        self.root.join(id.to_string())
    }

    #[cfg(test)]
    fn prune_at(&self, now: SystemTime) -> Result<(), HistoryError> {
        let mut state = self.lock()?;
        prune_expired(&mut state, self.limits, now)?;
        prune_total(&mut state, self.limits, 0)?;
        cleanup_empty_sessions(&mut state, &self.root)
    }
}

#[must_use]
pub fn default_terminal_history_path() -> PathBuf {
    let home = std::env::var_os("HOME").map_or_else(|| PathBuf::from("."), PathBuf::from);
    home.join(".local/state/swarm/terminal-history")
}

enum PageStart {
    Position {
        segment: usize,
        record: usize,
        reset: bool,
    },
    BeyondRetained,
}

fn page_start(segments: &[(u64, File)], cursor: Option<&HistoryCursor>) -> PageStart {
    let Some(cursor) = cursor else {
        return PageStart::Position {
            segment: 0,
            record: 0,
            reset: false,
        };
    };
    if let Some(segment) = segments
        .iter()
        .position(|(index, _)| *index == cursor.segment)
    {
        return PageStart::Position {
            segment,
            record: cursor.record as usize,
            reset: false,
        };
    }
    if cursor.segment > segments.last().expect("segments are non-empty").0 {
        PageStart::BeyondRetained
    } else {
        PageStart::Position {
            segment: 0,
            record: 0,
            reset: true,
        }
    }
}

fn close_current(state: &mut StoreState, id: WorkerSessionId) -> Result<(), HistoryError> {
    let session = state
        .sessions
        .get_mut(&id)
        .ok_or(HistoryError::SessionNotFound)?;
    if let Some(current) = session.current.take() {
        current.file.sync_data()?;
    }
    Ok(())
}

fn append_record_locked(
    state: &mut StoreState,
    root: &Path,
    id: WorkerSessionId,
    limits: HistoryLimits,
    record: &[u8],
    payload_bytes: u64,
    now: SystemTime,
) -> Result<HistoryAppendOutcome, HistoryError> {
    let record_bytes = record.len() as u64;
    prune_expired(state, limits, now)?;
    prune_session(state, id, limits, record_bytes)?;
    prune_total(state, limits, record_bytes)?;
    cleanup_empty_sessions(state, root)?;
    let session_bytes = state
        .sessions
        .get(&id)
        .ok_or(HistoryError::SessionNotFound)?
        .retained_bytes();
    if session_bytes.saturating_add(record_bytes) > limits.max_session_bytes
        || state.retained_bytes.saturating_add(record_bytes) > limits.max_total_bytes
    {
        state.dropped_records = state.dropped_records.saturating_add(1);
        state.dropped_bytes = state.dropped_bytes.saturating_add(payload_bytes);
        return Ok(HistoryAppendOutcome::DroppedAtCapacity);
    }

    ensure_current(state, root, id, now)?;
    let previous_size = state
        .sessions
        .get(&id)
        .and_then(|session| session.segments.back())
        .map_or(0, |segment| segment.bytes);
    let write_result = {
        let session = state
            .sessions
            .get_mut(&id)
            .ok_or(HistoryError::SessionNotFound)?;
        let current = session
            .current
            .as_mut()
            .ok_or(HistoryError::SessionNotFound)?;
        current.file.write_all(record)
    };
    if let Err(error) = write_result {
        let session = state
            .sessions
            .get_mut(&id)
            .ok_or(HistoryError::SessionNotFound)?;
        if let Some(current) = session.current.as_mut() {
            let _ = current.file.set_len(previous_size);
            let _ = current.file.seek(SeekFrom::End(0));
        }
        return Err(HistoryError::Io(error));
    }

    let session = state
        .sessions
        .get_mut(&id)
        .ok_or(HistoryError::SessionNotFound)?;
    let current = session
        .current
        .as_mut()
        .ok_or(HistoryError::SessionNotFound)?;
    current.unsynced_bytes = current.unsynced_bytes.saturating_add(record_bytes);
    if current.unsynced_bytes >= SYNC_INTERVAL_BYTES {
        current.file.sync_data()?;
        current.unsynced_bytes = 0;
    }
    let segment = session
        .segments
        .back_mut()
        .ok_or(HistoryError::SessionNotFound)?;
    segment.bytes = segment.bytes.saturating_add(record_bytes);
    segment.modified = now;
    state.retained_bytes = state.retained_bytes.saturating_add(record_bytes);
    Ok(HistoryAppendOutcome::Persisted)
}

fn ensure_current(
    state: &mut StoreState,
    root: &Path,
    id: WorkerSessionId,
    now: SystemTime,
) -> Result<(), HistoryError> {
    let session = state
        .sessions
        .get_mut(&id)
        .ok_or(HistoryError::SessionNotFound)?;
    if session.current.is_some() {
        return Ok(());
    }
    let index = session.next_segment;
    session.next_segment = session.next_segment.saturating_add(1);
    let path = root.join(id.to_string()).join(format!("{index:020}.swh"));
    let file = OpenOptions::new()
        .create_new(true)
        .read(true)
        .append(true)
        .open(&path)?;
    secure_file(&path)?;
    session.segments.push_back(Segment {
        index,
        path,
        bytes: 0,
        modified: now,
    });
    session.current = Some(CurrentSegment {
        index,
        file,
        unsynced_bytes: 0,
    });
    Ok(())
}

fn prune_expired(
    state: &mut StoreState,
    limits: HistoryLimits,
    now: SystemTime,
) -> Result<(), HistoryError> {
    let age = Duration::from_secs(limits.max_age_seconds);
    let candidates = state
        .sessions
        .iter()
        .flat_map(|(id, session)| {
            let current = session.current.as_ref().map(|segment| segment.index);
            session
                .segments
                .iter()
                .filter(move |segment| Some(segment.index) != current)
                .filter(move |segment| {
                    now.duration_since(segment.modified)
                        .is_ok_and(|elapsed| elapsed > age)
                })
                .map(move |segment| (*id, segment.index))
        })
        .collect::<Vec<_>>();
    for (id, index) in candidates {
        remove_segment(state, id, index)?;
    }
    Ok(())
}

fn prune_session(
    state: &mut StoreState,
    id: WorkerSessionId,
    limits: HistoryLimits,
    required: u64,
) -> Result<(), HistoryError> {
    loop {
        let session = state
            .sessions
            .get(&id)
            .ok_or(HistoryError::SessionNotFound)?;
        if session.retained_bytes().saturating_add(required) <= limits.max_session_bytes {
            return Ok(());
        }
        let current = session.current.as_ref().map(|segment| segment.index);
        let Some(index) = session
            .segments
            .iter()
            .find(|segment| Some(segment.index) != current)
            .map(|segment| segment.index)
        else {
            return Ok(());
        };
        remove_segment(state, id, index)?;
    }
}

fn prune_total(
    state: &mut StoreState,
    limits: HistoryLimits,
    required: u64,
) -> Result<(), HistoryError> {
    if state.retained_bytes.saturating_add(required) <= limits.max_total_bytes {
        return Ok(());
    }
    let mut candidates = state
        .sessions
        .iter()
        .flat_map(|(id, session)| {
            let current = session.current.as_ref().map(|segment| segment.index);
            session
                .segments
                .iter()
                .filter(move |segment| Some(segment.index) != current)
                .map(move |segment| (segment.modified, *id, segment.index))
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|candidate| candidate.0);
    for (_, id, index) in candidates {
        if state.retained_bytes.saturating_add(required) <= limits.max_total_bytes {
            break;
        }
        remove_segment(state, id, index)?;
    }
    Ok(())
}

fn remove_segment(
    state: &mut StoreState,
    id: WorkerSessionId,
    index: u64,
) -> Result<(), HistoryError> {
    let session = state
        .sessions
        .get_mut(&id)
        .ok_or(HistoryError::SessionNotFound)?;
    let Some(position) = session
        .segments
        .iter()
        .position(|segment| segment.index == index)
    else {
        return Ok(());
    };
    let segment = session
        .segments
        .remove(position)
        .ok_or(HistoryError::SessionNotFound)?;
    fs::remove_file(&segment.path)?;
    state.retained_bytes = state.retained_bytes.saturating_sub(segment.bytes);
    Ok(())
}

fn cleanup_empty_sessions(state: &mut StoreState, root: &Path) -> Result<(), HistoryError> {
    let empty = state
        .sessions
        .iter()
        .filter(|(_, session)| !session.active && session.segments.is_empty())
        .map(|(id, _)| *id)
        .collect::<Vec<_>>();
    for id in empty {
        match fs::remove_dir(root.join(id.to_string())) {
            Ok(()) => {
                state.sessions.remove(&id);
            }
            Err(ref error) if error.kind() == std::io::ErrorKind::NotFound => {
                state.sessions.remove(&id);
            }
            Err(ref error) if error.kind() == std::io::ErrorKind::DirectoryNotEmpty => {
                state.sessions.remove(&id);
            }
            Err(error) => return Err(HistoryError::Io(error)),
        }
    }
    Ok(())
}

fn scan_store(root: &Path, limits: HistoryLimits) -> Result<StoreState, HistoryError> {
    let mut state = StoreState::default();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if !file_type.is_dir() || file_type.is_symlink() {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let Ok(id) = WorkerSessionId::from_str(&name) else {
            continue;
        };
        secure_directory(&entry.path())?;
        let mut paths = fs::read_dir(entry.path())?
            .filter_map(Result::ok)
            .map(|child| child.path())
            .filter(|path| path.extension().is_some_and(|extension| extension == "swh"))
            .collect::<Vec<_>>();
        paths.sort();
        let mut session = SessionHistory::default();
        for path in paths {
            if fs::symlink_metadata(&path)?.file_type().is_symlink() {
                continue;
            }
            secure_file(&path)?;
            let index = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .and_then(|stem| stem.parse::<u64>().ok());
            let Some(index) = index else {
                continue;
            };
            let recovery = recover_segment(&path, limits.max_record_bytes)?;
            state.recovered_truncated_bytes = state
                .recovered_truncated_bytes
                .saturating_add(recovery.truncated_bytes);
            if recovery.corrupt {
                state.recovered_corrupt_segments =
                    state.recovered_corrupt_segments.saturating_add(1);
            }
            let modified = fs::metadata(&path)?.modified().unwrap_or(UNIX_EPOCH);
            state.retained_bytes = state.retained_bytes.saturating_add(recovery.valid_bytes);
            session.segments.push_back(Segment {
                index,
                path,
                bytes: recovery.valid_bytes,
                modified,
            });
            session.next_segment = session.next_segment.max(index.saturating_add(1));
        }
        if session.segments.is_empty() {
            let _ = fs::remove_dir(entry.path());
        } else {
            state.sessions.insert(id, session);
        }
    }
    Ok(state)
}

struct SegmentRecovery {
    valid_bytes: u64,
    truncated_bytes: u64,
    corrupt: bool,
}

fn recover_segment(path: &Path, max_record_bytes: u64) -> Result<SegmentRecovery, HistoryError> {
    let original_bytes = fs::metadata(path)?.len();
    let mut file = OpenOptions::new().read(true).write(true).open(path)?;
    let mut valid_bytes = 0_u64;
    let mut corrupt = false;
    let mut first_record = true;
    loop {
        let mut header = [0_u8; RECORD_HEADER_BYTES];
        let read = read_up_to(&mut file, &mut header)?;
        if read == 0 {
            break;
        }
        if read != RECORD_HEADER_BYTES {
            corrupt = true;
            break;
        }
        let Some((kind, sequence, timestamp_millis, payload_len, checksum)) =
            decode_header(&header)
        else {
            corrupt = true;
            break;
        };
        if u64::from(payload_len) > max_record_bytes {
            corrupt = true;
            break;
        }
        let mut payload = vec![0_u8; payload_len as usize];
        if read_up_to(&mut file, &mut payload)? != payload.len() {
            corrupt = true;
            break;
        }
        if (first_record && kind != CHECKPOINT_RECORD)
            || checksum != record_checksum(kind, sequence, timestamp_millis, &payload)
            || decode_record(kind, sequence, timestamp_millis, payload).is_none()
        {
            corrupt = true;
            break;
        }
        valid_bytes =
            valid_bytes.saturating_add(RECORD_HEADER_BYTES as u64 + u64::from(payload_len));
        first_record = false;
    }
    let truncated_bytes = original_bytes.saturating_sub(valid_bytes);
    if truncated_bytes > 0 {
        file.set_len(valid_bytes)?;
        file.sync_data()?;
    }
    Ok(SegmentRecovery {
        valid_bytes,
        truncated_bytes,
        corrupt,
    })
}

fn read_records(
    file: &mut File,
    max_record_bytes: u64,
) -> Result<Vec<HistoryRecord>, HistoryError> {
    let mut records = Vec::new();
    let mut first_record = true;
    loop {
        let mut header = [0_u8; RECORD_HEADER_BYTES];
        let read = read_up_to(file, &mut header)?;
        if read == 0 {
            break;
        }
        if read != RECORD_HEADER_BYTES {
            break;
        }
        let Some((kind, sequence, timestamp_millis, payload_len, checksum)) =
            decode_header(&header)
        else {
            break;
        };
        if u64::from(payload_len) > max_record_bytes {
            break;
        }
        let mut payload = vec![0_u8; payload_len as usize];
        if (first_record && kind != CHECKPOINT_RECORD)
            || read_up_to(file, &mut payload)? != payload.len()
            || checksum != record_checksum(kind, sequence, timestamp_millis, &payload)
        {
            break;
        }
        let Some(record) = decode_record(kind, sequence, timestamp_millis, payload) else {
            break;
        };
        records.push(record);
        first_record = false;
    }
    Ok(records)
}

fn encode_record(kind: u8, sequence: u64, timestamp: SystemTime, bytes: &[u8]) -> Vec<u8> {
    let timestamp_millis = u64::try_from(
        timestamp
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
    )
    .unwrap_or(u64::MAX);
    let payload_len = u32::try_from(bytes.len()).expect("validated record length fits u32");
    let checksum = record_checksum(kind, sequence, timestamp_millis, bytes);
    let mut record = Vec::with_capacity(RECORD_HEADER_BYTES + bytes.len());
    record.extend_from_slice(RECORD_MAGIC);
    record.push(RECORD_VERSION);
    record.push(kind);
    record.extend_from_slice(&0_u16.to_le_bytes());
    record.extend_from_slice(&sequence.to_le_bytes());
    record.extend_from_slice(&timestamp_millis.to_le_bytes());
    record.extend_from_slice(&payload_len.to_le_bytes());
    record.extend_from_slice(&checksum.to_le_bytes());
    record.extend_from_slice(bytes);
    record
}

fn decode_header(header: &[u8; RECORD_HEADER_BYTES]) -> Option<(u8, u64, u64, u32, u32)> {
    if &header[..4] != RECORD_MAGIC
        || header[4] != RECORD_VERSION
        || !matches!(header[5], OUTPUT_RECORD | CHECKPOINT_RECORD)
        || header[6..8] != [0, 0]
    {
        return None;
    }
    Some((
        header[5],
        u64::from_le_bytes(header[8..16].try_into().ok()?),
        u64::from_le_bytes(header[16..24].try_into().ok()?),
        u32::from_le_bytes(header[24..28].try_into().ok()?),
        u32::from_le_bytes(header[28..32].try_into().ok()?),
    ))
}

fn encode_checkpoint(snapshot: &TerminalSnapshot) -> Vec<u8> {
    let mut payload = Vec::with_capacity(5 + snapshot.bytes.len());
    payload.extend_from_slice(&snapshot.rows.to_le_bytes());
    payload.extend_from_slice(&snapshot.columns.to_le_bytes());
    payload.push(u8::from(snapshot.truncated));
    payload.extend_from_slice(&snapshot.bytes);
    payload
}

fn decode_record(
    kind: u8,
    sequence: u64,
    timestamp_millis: u64,
    payload: Vec<u8>,
) -> Option<HistoryRecord> {
    match kind {
        OUTPUT_RECORD => Some(HistoryRecord::Output {
            sequence,
            timestamp_millis,
            bytes: payload,
        }),
        CHECKPOINT_RECORD if payload.len() >= 5 => Some(HistoryRecord::Checkpoint {
            timestamp_millis,
            snapshot: TerminalSnapshot {
                sequence,
                rows: u16::from_le_bytes(payload[..2].try_into().ok()?),
                columns: u16::from_le_bytes(payload[2..4].try_into().ok()?),
                truncated: payload[4] != 0,
                bytes: payload[5..].to_vec(),
            },
        }),
        _ => None,
    }
}

fn record_checksum(kind: u8, sequence: u64, timestamp_millis: u64, bytes: &[u8]) -> u32 {
    let mut checksum = 0xffff_ffff_u32;
    for byte in [kind]
        .into_iter()
        .chain(sequence.to_le_bytes())
        .chain(timestamp_millis.to_le_bytes())
        .chain((bytes.len() as u64).to_le_bytes())
        .chain(bytes.iter().copied())
    {
        checksum ^= u32::from(byte);
        for _ in 0..8 {
            let mask = 0_u32.wrapping_sub(checksum & 1);
            checksum = (checksum >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !checksum
}

fn read_up_to(reader: &mut impl Read, buffer: &mut [u8]) -> Result<usize, std::io::Error> {
    let mut read = 0;
    while read < buffer.len() {
        match reader.read(&mut buffer[read..])? {
            0 => break,
            count => read += count,
        }
    }
    Ok(read)
}

#[cfg(unix)]
fn secure_directory(path: &Path) -> Result<(), HistoryError> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    if let Ok(metadata) = fs::symlink_metadata(path)
        && metadata.file_type().is_symlink()
    {
        return Err(HistoryError::InsecureDirectory(path.to_path_buf()));
    }
    fs::create_dir_all(path)?;
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || metadata.uid() != nix::unistd::Uid::effective().as_raw()
    {
        return Err(HistoryError::InsecureDirectory(path.to_path_buf()));
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(not(unix))]
fn secure_directory(path: &Path) -> Result<(), HistoryError> {
    if let Ok(metadata) = fs::symlink_metadata(path)
        && metadata.file_type().is_symlink()
    {
        return Err(HistoryError::InsecureDirectory(path.to_path_buf()));
    }
    fs::create_dir_all(path)?;
    Ok(())
}

#[cfg(unix)]
fn secure_file(path: &Path) -> Result<(), HistoryError> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.uid() != nix::unistd::Uid::effective().as_raw()
    {
        return Err(HistoryError::InsecureDirectory(path.to_path_buf()));
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn secure_file(_path: &Path) -> Result<(), HistoryError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tempfile::TempDir;

    use super::*;
    use crate::{CanonicalTerminalState, JournalLimits, TerminalSize};

    fn limits() -> HistoryLimits {
        HistoryLimits::new(128, 192, 384, 768, 60)
    }

    fn store(temp: &TempDir) -> HistoryStore {
        HistoryStore::open(temp.path().join("history"), limits()).unwrap()
    }

    fn checkpoint(sequence: u64, bytes: &[u8]) -> TerminalSnapshot {
        TerminalSnapshot {
            sequence,
            rows: 24,
            columns: 80,
            truncated: false,
            bytes: bytes.to_vec(),
        }
    }

    fn start(store: &HistoryStore, id: WorkerSessionId) {
        store.start_session(id).unwrap();
        store.append_checkpoint(id, &checkpoint(0, b"")).unwrap();
    }

    fn append_output(store: &HistoryStore, id: WorkerSessionId, sequence: u64, bytes: &[u8]) {
        if store.append(id, sequence, bytes).unwrap() == HistoryAppendOutcome::CheckpointRequired {
            store
                .append_checkpoint(id, &checkpoint(sequence, bytes))
                .unwrap();
        }
    }

    #[test]
    fn appends_and_reads_exact_sequenced_bytes() {
        let temp = TempDir::new().unwrap();
        let store = store(&temp);
        let id = WorkerSessionId::new();
        start(&store, id);
        append_output(&store, id, 7, b"hello");
        append_output(&store, id, 8, b" world");

        let records = store.read_session(id).unwrap();
        assert_eq!(records.len(), 3);
        assert!(matches!(records[0], HistoryRecord::Checkpoint { .. }));
        assert!(matches!(
            records[1],
            HistoryRecord::Output { sequence: 7, .. }
        ));
        assert!(matches!(
            records[2],
            HistoryRecord::Output { sequence: 8, .. }
        ));
    }

    #[test]
    fn history_pages_are_bounded_and_resume_without_duplicate_records() {
        let temp = TempDir::new().unwrap();
        let store = store(&temp);
        let id = WorkerSessionId::new();
        start(&store, id);
        append_output(&store, id, 7, b"hello");
        append_output(&store, id, 8, b"world");

        let first = store.page_with_bounds(id, None, 128, 1).unwrap();
        assert_eq!(first.records.len(), 1);
        assert!(first.has_more);
        assert!(matches!(first.records[0], HistoryRecord::Checkpoint { .. }));
        let second = store
            .page_with_bounds(id, first.next_cursor, 128, 1)
            .unwrap();
        assert!(matches!(
            second.records[0],
            HistoryRecord::Output { sequence: 7, .. }
        ));
        let third = store
            .page_with_bounds(id, second.next_cursor, 128, 1)
            .unwrap();
        assert!(matches!(
            third.records[0],
            HistoryRecord::Output { sequence: 8, .. }
        ));
        assert!(!third.has_more);
    }

    #[test]
    fn evicted_history_cursor_resets_to_a_retained_checkpoint() {
        let temp = TempDir::new().unwrap();
        let store = store(&temp);
        let id = WorkerSessionId::new();
        start(&store, id);
        let original = store.page_with_bounds(id, None, 128, 1).unwrap();
        let stale_cursor = original.next_cursor;
        for sequence in 1..=20 {
            append_output(&store, id, sequence, &[b'x'; 80]);
        }

        let recovered = store.page_with_bounds(id, stale_cursor, 128, 1).unwrap();
        assert!(recovered.reset);
        assert!(matches!(
            recovered.records.first(),
            Some(HistoryRecord::Checkpoint { .. })
        ));
    }

    #[test]
    fn retained_checkpoint_and_output_records_rebuild_canonical_state() {
        let temp = TempDir::new().unwrap();
        let limits = HistoryLimits::new(4096, 4608, 9216, 9216, 60);
        let store = HistoryStore::open(temp.path().join("history"), limits).unwrap();
        let id = WorkerSessionId::new();
        let mut canonical =
            CanonicalTerminalState::new(JournalLimits::new(1024, 32), TerminalSize::new(4, 20));
        store.start_session(id).unwrap();
        store.append_checkpoint(id, &canonical.snapshot()).unwrap();
        for line in 0..100 {
            let output = format!("line-{line:03}\r\n").into_bytes();
            let sequence = canonical.push(output.clone());
            if store.append(id, sequence, &output).unwrap()
                == HistoryAppendOutcome::CheckpointRequired
            {
                store.append_checkpoint(id, &canonical.snapshot()).unwrap();
            }
        }

        let mut replay: Option<vt100::Parser> = None;
        for record in store.read_session(id).unwrap() {
            match record {
                HistoryRecord::Checkpoint { snapshot, .. } => {
                    let mut parser = vt100::Parser::new(
                        snapshot.rows,
                        snapshot.columns,
                        crate::CANONICAL_SCROLLBACK_ROWS,
                    );
                    parser.process(&snapshot.bytes);
                    replay = Some(parser);
                }
                HistoryRecord::Output { bytes, .. } => {
                    replay.as_mut().unwrap().process(&bytes);
                }
            }
        }
        let replay = replay.expect("retained history must begin with a checkpoint");
        let expected_snapshot = canonical.snapshot();
        let mut expected = vt100::Parser::new(
            expected_snapshot.rows,
            expected_snapshot.columns,
            crate::CANONICAL_SCROLLBACK_ROWS,
        );
        expected.process(&expected_snapshot.bytes);
        assert_eq!(replay.screen().contents(), expected.screen().contents());
        let mut replay_history = replay.screen().clone();
        replay_history.set_scrollback(usize::MAX);
        let mut expected_history = expected.screen().clone();
        expected_history.set_scrollback(usize::MAX);
        assert_eq!(replay_history.contents(), expected_history.contents());
    }

    #[test]
    fn sustained_output_stays_inside_session_and_store_bounds() {
        let temp = TempDir::new().unwrap();
        let store = store(&temp);
        let id = WorkerSessionId::new();
        start(&store, id);
        for sequence in 1..=10_000 {
            append_output(&store, id, sequence, &[b'x'; 80]);
            let diagnostics = store.diagnostics().unwrap();
            assert!(diagnostics.retained_bytes <= limits().max_session_bytes);
            assert!(diagnostics.retained_bytes <= limits().max_total_bytes);
        }
        let records = store.read_session(id).unwrap();
        assert_eq!(records.last().unwrap().sequence(), 10_000);
        assert!(matches!(
            records.first(),
            Some(HistoryRecord::Checkpoint { .. })
        ));
        let paths = {
            let state = store.lock().unwrap();
            state.sessions[&id]
                .segments
                .iter()
                .map(|segment| segment.path.clone())
                .collect::<Vec<_>>()
        };
        assert!(paths.len() > 1);
        for path in paths {
            let mut file = File::open(path).unwrap();
            let records = read_records(&mut file, limits().max_record_bytes).unwrap();
            assert!(matches!(
                records.first(),
                Some(HistoryRecord::Checkpoint { .. })
            ));
        }
    }

    #[test]
    fn empty_finished_sessions_do_not_accumulate_metadata() {
        let temp = TempDir::new().unwrap();
        let store = store(&temp);
        let id = WorkerSessionId::new();
        store.start_session(id).unwrap();
        store.finish_session(id).unwrap();
        assert_eq!(store.diagnostics().unwrap().session_count, 0);
        assert!(!store.session_path(id).exists());
    }

    #[test]
    fn unknown_files_do_not_create_unbounded_in_memory_session_entries() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("history");
        fs::create_dir(&root).unwrap();
        let id = WorkerSessionId::new();
        let session = root.join(id.to_string());
        fs::create_dir(&session).unwrap();
        fs::write(session.join("unknown.file"), b"not terminal history").unwrap();

        let store = HistoryStore::open(&root, limits()).unwrap();
        assert_eq!(store.diagnostics().unwrap().session_count, 0);
    }

    #[test]
    fn total_bound_evicts_old_closed_segments_across_sessions() {
        let temp = TempDir::new().unwrap();
        let limits = HistoryLimits::new(128, 192, 384, 440, 60);
        let store = HistoryStore::open(temp.path().join("history"), limits).unwrap();
        let first = WorkerSessionId::new();
        let second = WorkerSessionId::new();
        start(&store, first);
        for sequence in 1..=4 {
            append_output(&store, first, sequence, &[b'a'; 80]);
        }
        store.finish_session(first).unwrap();
        start(&store, second);
        for sequence in 1..=4 {
            append_output(&store, second, sequence, &[b'b'; 80]);
        }
        assert!(store.diagnostics().unwrap().retained_bytes <= limits.max_total_bytes);
    }

    #[test]
    fn active_segments_drop_new_history_instead_of_exceeding_total_bound() {
        let temp = TempDir::new().unwrap();
        let limits = HistoryLimits::new(128, 192, 192, 192, 60);
        let store = HistoryStore::open(temp.path().join("history"), limits).unwrap();
        let first = WorkerSessionId::new();
        let second = WorkerSessionId::new();
        start(&store, first);
        start(&store, second);
        assert_eq!(
            store.append(first, 1, &[b'a'; 80]).unwrap(),
            HistoryAppendOutcome::Persisted
        );
        assert_eq!(
            store.append(second, 1, &[b'b'; 80]).unwrap(),
            HistoryAppendOutcome::DroppedAtCapacity
        );
        let diagnostics = store.diagnostics().unwrap();
        assert!(diagnostics.retained_bytes <= limits.max_total_bytes);
        assert_eq!(diagnostics.dropped_records, 1);
        assert_eq!(diagnostics.dropped_bytes, 80);
    }

    #[test]
    fn truncated_tail_recovers_every_complete_record() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("history");
        let id = WorkerSessionId::new();
        {
            let store = HistoryStore::open(&root, limits()).unwrap();
            start(&store, id);
            append_output(&store, id, 1, b"complete");
            append_output(&store, id, 2, b"torn-tail");
            store.finish_session(id).unwrap();
        }
        let segment = fs::read_dir(root.join(id.to_string()))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let original = fs::metadata(&segment).unwrap().len();
        OpenOptions::new()
            .write(true)
            .open(&segment)
            .unwrap()
            .set_len(original - 5)
            .unwrap();

        let recovered = HistoryStore::open(&root, limits()).unwrap();
        assert_eq!(recovered.read_session(id).unwrap().len(), 2);
        assert!(recovered.diagnostics().unwrap().recovered_truncated_bytes > 0);
    }

    #[test]
    fn checksum_failure_discards_only_the_untrustworthy_tail() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("history");
        let id = WorkerSessionId::new();
        {
            let store = HistoryStore::open(&root, limits()).unwrap();
            start(&store, id);
            append_output(&store, id, 1, b"trusted");
            append_output(&store, id, 2, b"corrupt-me");
            store.finish_session(id).unwrap();
        }
        let segment = fs::read_dir(root.join(id.to_string()))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&segment)
            .unwrap();
        file.seek(SeekFrom::End(-1)).unwrap();
        file.write_all(b"!").unwrap();

        let recovered = HistoryStore::open(&root, limits()).unwrap();
        let records = recovered.read_session(id).unwrap();
        assert_eq!(records.len(), 2);
        assert!(matches!(records[0], HistoryRecord::Checkpoint { .. }));
        assert!(matches!(
            &records[1],
            HistoryRecord::Output { bytes, .. } if bytes == b"trusted"
        ));
        assert_eq!(
            recovered.diagnostics().unwrap().recovered_corrupt_segments,
            1
        );
    }

    #[test]
    fn age_bound_removes_closed_segments() {
        let temp = TempDir::new().unwrap();
        let store = store(&temp);
        let id = WorkerSessionId::new();
        let written = UNIX_EPOCH + Duration::from_secs(100);
        store.start_session(id).unwrap();
        store
            .append_checkpoint_at(id, &checkpoint(0, b"old"), written)
            .unwrap();
        store.finish_session(id).unwrap();
        store
            .prune_at(written + Duration::from_secs(limits().max_age_seconds + 1))
            .unwrap();
        assert_eq!(store.diagnostics().unwrap().retained_bytes, 0);
    }

    #[cfg(unix)]
    #[test]
    fn history_paths_are_private_to_the_current_user() {
        use std::os::unix::fs::PermissionsExt;

        let temp = TempDir::new().unwrap();
        let root = temp.path().join("history");
        let store = HistoryStore::open(&root, limits()).unwrap();
        let id = WorkerSessionId::new();
        start(&store, id);
        append_output(&store, id, 1, b"private");
        let segment = fs::read_dir(root.join(id.to_string()))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        assert_eq!(
            fs::metadata(&root).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(segment).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

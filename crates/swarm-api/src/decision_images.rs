//! Private files for images the operator sends with an answer or a question on
//! a request (ADR 0174).
//!
//! The bytes are written here before the answer is recorded, and the record is
//! what makes them part of it: a file whose answer was refused is removed at
//! once, and one left behind by a crash between the two is removed by the next
//! write. Nothing here is named by its content, and no byte, path or hash goes
//! to `SQLite` or a log (ADR 0021).
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime},
};

use axum::{
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use base64ct::{Base64, Encoding as _};
use swarm_domain::DecisionRequestId;
use swarm_persistence::{
    DECISION_IMAGE_MEDIA_TYPES, DecisionImage, MAX_DECISION_IMAGE_BYTES, MAX_IMAGES_PER_SUBMISSION,
    NewDecisionImage, TaskStore,
};
use tokio::sync::Mutex;

use crate::{ApiError, AppState};

/// The largest request body that can carry a full set of images, base64
/// encoded, with room for the answer or question beside them.
pub(crate) const MAX_BODY_WITH_IMAGES: usize = 22 * 1024 * 1024;
/// A file nothing records is removed once it is this old. Younger ones may
/// belong to an answer being recorded right now.
const UNRECORDED_GRACE: Duration = Duration::from_secs(10 * 60);

/// One image as the browser sends it: its declared type and base64 bytes.
#[derive(Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SentImage {
    pub(crate) media_type: String,
    pub(crate) data: String,
}

/// Says what was sent without the bytes, so a logged request carries no image.
impl std::fmt::Debug for SentImage {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SentImage")
            .field("media_type", &self.media_type)
            .field("encoded_bytes", &self.data.len())
            .finish()
    }
}

#[derive(Clone)]
pub(crate) struct DecisionImageStore {
    root: Arc<PathBuf>,
    write_lock: Arc<Mutex<()>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub(crate) enum DecisionImageError {
    #[error("send at most {MAX_IMAGES_PER_SUBMISSION} images with one answer or question")]
    TooMany,
    #[error("an image must be a PNG, JPEG, GIF or WebP of at most 3.75 MiB")]
    Invalid,
    #[error("image storage for requests is not configured")]
    Unconfigured,
    #[error("image storage for requests is unavailable")]
    Unavailable,
    #[error("that image was not found")]
    NotFound,
}

impl DecisionImageStore {
    #[must_use]
    pub(crate) fn new(root: PathBuf) -> Self {
        Self {
            root: Arc::new(root),
            write_lock: Arc::new(Mutex::new(())),
        }
    }

    /// Checks every image, then writes each under a fresh id. Nothing is
    /// written when any one is refused.
    pub(crate) async fn write(
        &self,
        store: &TaskStore,
        images: &[SentImage],
    ) -> Result<Vec<NewDecisionImage>, DecisionImageError> {
        if images.is_empty() {
            return Ok(Vec::new());
        }
        if images.len() > MAX_IMAGES_PER_SUBMISSION {
            return Err(DecisionImageError::TooMany);
        }
        let decoded = images.iter().map(decode).collect::<Result<Vec<_>, _>>()?;
        let _guard = self.write_lock.lock().await;
        tokio::fs::create_dir_all(self.root.as_ref())
            .await
            .map_err(|_| DecisionImageError::Unavailable)?;
        secure_directory(self.root.as_ref())?;
        // A crash between writing and recording leaves files nothing names.
        // They are bounded by what one request can carry, and this is where
        // they go.
        let kept = store
            .kept_decision_image_ids()
            .map_err(|_| DecisionImageError::Unavailable)?;
        remove_unrecorded(self.root.as_ref(), &kept, UNRECORDED_GRACE).await?;
        let mut written = Vec::with_capacity(decoded.len());
        for (media_type, bytes) in decoded {
            let image = NewDecisionImage {
                id: uuid::Uuid::now_v7(),
                media_type: media_type.to_owned(),
                byte_len: bytes.len() as u64,
            };
            if let Err(error) = write_private(&self.path(image.id, media_type), &bytes).await {
                self.remove(&written).await;
                return Err(error);
            }
            written.push(image);
        }
        Ok(written)
    }

    /// Removes files written for an answer that was not recorded.
    pub(crate) async fn remove(&self, images: &[NewDecisionImage]) {
        for image in images {
            let _ = tokio::fs::remove_file(self.path(image.id, &image.media_type)).await;
        }
    }

    pub(crate) async fn read(&self, image: &DecisionImage) -> Result<Vec<u8>, DecisionImageError> {
        let path = self.path(image.id, &image.media_type);
        let metadata = tokio::fs::symlink_metadata(&path).await.map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                DecisionImageError::NotFound
            } else {
                DecisionImageError::Unavailable
            }
        })?;
        if !metadata.file_type().is_file() || metadata.len() != image.byte_len {
            return Err(DecisionImageError::NotFound);
        }
        let bytes = tokio::fs::read(&path)
            .await
            .map_err(|_| DecisionImageError::Unavailable)?;
        if !signature_matches(&image.media_type, &bytes) {
            return Err(DecisionImageError::NotFound);
        }
        Ok(bytes)
    }

    fn path(&self, id: uuid::Uuid, media_type: &str) -> PathBuf {
        self.root.join(format!("{id}.{}", extension(media_type)))
    }
}

fn decode(image: &SentImage) -> Result<(&'static str, Vec<u8>), DecisionImageError> {
    let declared = image.media_type.trim().to_ascii_lowercase();
    let media_type = DECISION_IMAGE_MEDIA_TYPES
        .into_iter()
        .find(|known| *known == declared)
        .ok_or(DecisionImageError::Invalid)?;
    // Refuse an oversized one before decoding it, not after.
    let encoded_limit = MAX_DECISION_IMAGE_BYTES.div_ceil(3) * 4;
    if image.data.is_empty() || image.data.len() as u64 > encoded_limit {
        return Err(DecisionImageError::Invalid);
    }
    let bytes = Base64::decode_vec(&image.data).map_err(|_| DecisionImageError::Invalid)?;
    if bytes.is_empty()
        || bytes.len() as u64 > MAX_DECISION_IMAGE_BYTES
        || !signature_matches(media_type, &bytes)
    {
        return Err(DecisionImageError::Invalid);
    }
    Ok((media_type, bytes))
}

fn signature_matches(media_type: &str, bytes: &[u8]) -> bool {
    match media_type {
        "image/png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "image/jpeg" => bytes.starts_with(&[0xff, 0xd8, 0xff]),
        "image/gif" => bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"),
        "image/webp" => bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP",
        _ => false,
    }
}

fn extension(media_type: &str) -> &'static str {
    match media_type {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        _ => "webp",
    }
}

/// Removes files no record names, once they are older than `grace`.
async fn remove_unrecorded(
    root: &Path,
    kept: &HashSet<uuid::Uuid>,
    grace: Duration,
) -> Result<(), DecisionImageError> {
    let cutoff = SystemTime::now()
        .checked_sub(grace)
        .unwrap_or(SystemTime::UNIX_EPOCH);
    let mut entries = tokio::fs::read_dir(root)
        .await
        .map_err(|_| DecisionImageError::Unavailable)?;
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|_| DecisionImageError::Unavailable)?
    {
        let recorded = entry
            .file_name()
            .to_str()
            .and_then(|name| name.split_once('.'))
            .and_then(|(id, _)| id.parse::<uuid::Uuid>().ok())
            .is_some_and(|id| kept.contains(&id));
        if recorded {
            continue;
        }
        let old = entry
            .metadata()
            .await
            .ok()
            .and_then(|metadata| metadata.modified().ok())
            .is_some_and(|modified| modified < cutoff);
        if old {
            let _ = tokio::fs::remove_file(entry.path()).await;
        }
    }
    Ok(())
}

async fn write_private(path: &Path, bytes: &[u8]) -> Result<(), DecisionImageError> {
    #[cfg(unix)]
    {
        use tokio::io::AsyncWriteExt;
        let mut options = tokio::fs::OpenOptions::new();
        options.write(true).create_new(true).mode(0o600);
        let mut file = options
            .open(path)
            .await
            .map_err(|_| DecisionImageError::Unavailable)?;
        if file.write_all(bytes).await.is_err() || file.sync_all().await.is_err() {
            drop(file);
            let _ = tokio::fs::remove_file(path).await;
            return Err(DecisionImageError::Unavailable);
        }
        Ok(())
    }
    #[cfg(not(unix))]
    tokio::fs::write(path, bytes)
        .await
        .map_err(|_| DecisionImageError::Unavailable)
}

fn secure_directory(path: &Path) -> Result<(), DecisionImageError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .map_err(|_| DecisionImageError::Unavailable)?;
    }
    Ok(())
}

pub(crate) fn decision_image_error(error: DecisionImageError) -> ApiError {
    let (status, code) = match error {
        DecisionImageError::TooMany => {
            (StatusCode::UNPROCESSABLE_ENTITY, "too_many_decision_images")
        }
        DecisionImageError::Invalid => (StatusCode::UNPROCESSABLE_ENTITY, "invalid_decision_image"),
        DecisionImageError::Unconfigured => (
            StatusCode::SERVICE_UNAVAILABLE,
            "decision_image_store_unconfigured",
        ),
        DecisionImageError::Unavailable => (
            StatusCode::SERVICE_UNAVAILABLE,
            "decision_image_store_unavailable",
        ),
        DecisionImageError::NotFound => (StatusCode::NOT_FOUND, "decision_image_not_found"),
    };
    ApiError::new(status, code, error.to_string())
}

/// Writes the images sent with an answer or question, before it is recorded.
pub(crate) async fn write_sent(
    state: &AppState,
    store: &TaskStore,
    images: &[SentImage],
) -> Result<Vec<NewDecisionImage>, ApiError> {
    if images.is_empty() {
        return Ok(Vec::new());
    }
    let files = state
        .decision_image_store
        .as_ref()
        .ok_or(DecisionImageError::Unconfigured)
        .map_err(decision_image_error)?;
    files
        .write(store, images)
        .await
        .map_err(decision_image_error)
}

/// Removes the files written for an answer or question that was not
/// recorded, or not recorded with these images.
pub(crate) async fn discard(state: &AppState, images: &[NewDecisionImage]) {
    if let Some(files) = state.decision_image_store.as_ref() {
        files.remove(images).await;
    }
}

/// One recorded image's bytes, with the headers that keep a browser from
/// reading it as anything but the image it is.
pub(crate) async fn image_response(
    state: &AppState,
    store: &TaskStore,
    decision: DecisionRequestId,
    image: uuid::Uuid,
) -> Result<Response, ApiError> {
    let recorded = store
        .decision_image(decision, image)
        .map_err(|error| crate::task_store_error(&error))?
        .ok_or(DecisionImageError::NotFound)
        .map_err(decision_image_error)?;
    let files = state
        .decision_image_store
        .as_ref()
        .ok_or(DecisionImageError::Unconfigured)
        .map_err(decision_image_error)?;
    let bytes = files.read(&recorded).await.map_err(decision_image_error)?;
    let mut headers = HeaderMap::new();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(&recorded.media_type)
            .map_err(|_| decision_image_error(DecisionImageError::Unavailable))?,
    );
    Ok((headers, bytes).into_response())
}

#[cfg(test)]
mod tests;

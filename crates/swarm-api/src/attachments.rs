use std::{
    fmt::Write,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime},
};

use sha2::{Digest, Sha256};
use tokio::sync::Mutex;

pub const MAX_ATTACHMENT_BYTES: usize = 8 * 1024 * 1024;
const MAX_ATTACHMENT_STORE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_ATTACHMENT_FILES: usize = 64;
const MAX_ATTACHMENT_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);

#[derive(Clone)]
pub struct AttachmentStore {
    root: Arc<PathBuf>,
    write_lock: Arc<Mutex<()>>,
}

#[derive(Clone, Copy, Debug, thiserror::Error)]
pub enum AttachmentError {
    #[error("unsupported image type")]
    UnsupportedType,
    #[error("image content did not match its declared type")]
    InvalidSignature,
    #[error("image must contain 1 to {MAX_ATTACHMENT_BYTES} bytes")]
    InvalidSize,
    #[error("private attachment storage is full")]
    Capacity,
    #[error("private attachment storage is unavailable")]
    Unavailable,
}

impl AttachmentStore {
    #[must_use]
    pub fn new(root: PathBuf) -> Self {
        Self {
            root: Arc::new(root),
            write_lock: Arc::new(Mutex::new(())),
        }
    }

    pub async fn save(&self, media_type: &str, bytes: &[u8]) -> Result<PathBuf, AttachmentError> {
        if bytes.is_empty() || bytes.len() > MAX_ATTACHMENT_BYTES {
            return Err(AttachmentError::InvalidSize);
        }
        let extension = validated_extension(media_type, bytes)?;
        let _guard = self.write_lock.lock().await;
        tokio::fs::create_dir_all(self.root.as_ref())
            .await
            .map_err(|_| AttachmentError::Unavailable)?;
        secure_directory(self.root.as_ref())?;
        prune_expired(self.root.as_ref()).await?;
        let digest = Sha256::digest(bytes);
        let name = format!("{}.{}", hex_prefix(&digest, 20), extension);
        let path = self.root.join(name);
        if tokio::fs::try_exists(&path)
            .await
            .map_err(|_| AttachmentError::Unavailable)?
        {
            return Ok(path);
        }
        let (files, retained_bytes) = store_usage(self.root.as_ref()).await?;
        if files >= MAX_ATTACHMENT_FILES
            || retained_bytes.saturating_add(bytes.len() as u64) > MAX_ATTACHMENT_STORE_BYTES
        {
            return Err(AttachmentError::Capacity);
        }
        write_private(&path, bytes).await?;
        Ok(path)
    }
}

fn validated_extension(media_type: &str, bytes: &[u8]) -> Result<&'static str, AttachmentError> {
    match media_type.split(';').next().unwrap_or_default().trim() {
        "image/png" if bytes.starts_with(b"\x89PNG\r\n\x1a\n") => Ok("png"),
        "image/jpeg" if bytes.starts_with(&[0xff, 0xd8, 0xff]) => Ok("jpg"),
        "image/webp" if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" => {
            Ok("webp")
        }
        "image/gif" if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") => Ok("gif"),
        "image/png" | "image/jpeg" | "image/webp" | "image/gif" => {
            Err(AttachmentError::InvalidSignature)
        }
        _ => Err(AttachmentError::UnsupportedType),
    }
}

fn hex_prefix(bytes: &[u8], length: usize) -> String {
    let mut result = String::with_capacity(length);
    for byte in bytes {
        let _ = write!(result, "{byte:02x}");
        if result.len() >= length {
            result.truncate(length);
            break;
        }
    }
    result
}

async fn store_usage(root: &Path) -> Result<(usize, u64), AttachmentError> {
    let mut entries = tokio::fs::read_dir(root)
        .await
        .map_err(|_| AttachmentError::Unavailable)?;
    let mut files = 0usize;
    let mut bytes = 0u64;
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|_| AttachmentError::Unavailable)?
    {
        let metadata = entry
            .metadata()
            .await
            .map_err(|_| AttachmentError::Unavailable)?;
        if metadata.is_file() {
            files = files.saturating_add(1);
            bytes = bytes.saturating_add(metadata.len());
        }
    }
    Ok((files, bytes))
}

async fn prune_expired(root: &Path) -> Result<(), AttachmentError> {
    let cutoff = SystemTime::now()
        .checked_sub(MAX_ATTACHMENT_AGE)
        .unwrap_or(SystemTime::UNIX_EPOCH);
    let mut entries = tokio::fs::read_dir(root)
        .await
        .map_err(|_| AttachmentError::Unavailable)?;
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|_| AttachmentError::Unavailable)?
    {
        let metadata = entry
            .metadata()
            .await
            .map_err(|_| AttachmentError::Unavailable)?;
        if metadata.is_file() && metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH) < cutoff {
            tokio::fs::remove_file(entry.path())
                .await
                .map_err(|_| AttachmentError::Unavailable)?;
        }
    }
    Ok(())
}

async fn write_private(path: &Path, bytes: &[u8]) -> Result<(), AttachmentError> {
    #[cfg(unix)]
    {
        use tokio::io::AsyncWriteExt;
        let mut options = tokio::fs::OpenOptions::new();
        options.write(true).create_new(true).mode(0o600);
        let mut file = options
            .open(path)
            .await
            .map_err(|_| AttachmentError::Unavailable)?;
        if file.write_all(bytes).await.is_err() || file.sync_all().await.is_err() {
            drop(file);
            let _ = tokio::fs::remove_file(path).await;
            return Err(AttachmentError::Unavailable);
        }
        Ok(())
    }
    #[cfg(not(unix))]
    tokio::fs::write(path, bytes)
        .await
        .map_err(|_| AttachmentError::Unavailable)
}

fn secure_directory(path: &Path) -> Result<(), AttachmentError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .map_err(|_| AttachmentError::Unavailable)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\nprivate-image";

    #[tokio::test]
    async fn writes_a_content_named_private_image() {
        let directory = tempfile::tempdir().unwrap();
        let store = AttachmentStore::new(directory.path().join("attachments"));

        let path = store.save("image/png", PNG).await.unwrap();

        assert_eq!(tokio::fs::read(&path).await.unwrap(), PNG);
        assert_eq!(
            path.extension().and_then(|value| value.to_str()),
            Some("png")
        );
        assert!(
            !path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .contains("private")
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[tokio::test]
    async fn rejects_spoofed_and_oversized_images() {
        let directory = tempfile::tempdir().unwrap();
        let store = AttachmentStore::new(directory.path().join("attachments"));

        assert!(matches!(
            store.save("image/png", b"not a png").await,
            Err(AttachmentError::InvalidSignature)
        ));
        assert!(matches!(
            store
                .save("image/png", &vec![0; MAX_ATTACHMENT_BYTES + 1])
                .await,
            Err(AttachmentError::InvalidSize)
        ));
    }
}

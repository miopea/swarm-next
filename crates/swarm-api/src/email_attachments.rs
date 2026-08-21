use std::{
    fmt::Write,
    path::{Path, PathBuf},
    sync::Arc,
};

use sha2::{Digest, Sha256};
use tokio::sync::Mutex;

pub(crate) const MAX_EMAIL_ATTACHMENT_BYTES: usize = 10 * 1024 * 1024;
/// The store's share of whatever disk it lives on. A fixed ceiling here would
/// fail the same way the private attachment store did: refusing a new file at a
/// fraction of a large disk, and claiming space that is gone on a full one.
const EMAIL_ATTACHMENT_DISK_PERCENT: u64 = 5;
const MAX_EMAIL_ATTACHMENT_FILES: usize = 2_048;

#[derive(Clone)]
pub(crate) struct EmailAttachmentStore {
    root: Arc<PathBuf>,
    write_lock: Arc<Mutex<()>>,
}

#[derive(Clone, Copy, Debug, thiserror::Error)]
pub(crate) enum EmailAttachmentError {
    #[error("email attachment must contain 1 to {MAX_EMAIL_ATTACHMENT_BYTES} bytes")]
    InvalidSize,
    #[error("email attachment content did not match its declared type")]
    InvalidSignature,
    #[error("private email attachment storage is full")]
    Capacity,
    #[error("private email attachment storage is unavailable")]
    Unavailable,
    #[error("private email attachment was not found")]
    NotFound,
}

impl EmailAttachmentStore {
    #[must_use]
    pub(crate) fn new(root: PathBuf) -> Self {
        Self {
            root: Arc::new(root),
            write_lock: Arc::new(Mutex::new(())),
        }
    }

    pub(crate) async fn save(
        &self,
        media_type: &str,
        bytes: &[u8],
    ) -> Result<String, EmailAttachmentError> {
        if bytes.is_empty() || bytes.len() > MAX_EMAIL_ATTACHMENT_BYTES {
            return Err(EmailAttachmentError::InvalidSize);
        }
        let (extension, normalized_type) = attachment_kind(media_type, bytes)?;
        let _guard = self.write_lock.lock().await;
        tokio::fs::create_dir_all(self.root.as_ref())
            .await
            .map_err(|_| EmailAttachmentError::Unavailable)?;
        secure_directory(self.root.as_ref())?;
        let digest = Sha256::digest(bytes);
        let name = format!("{}.{}", hex_prefix(&digest, 32), extension);
        let path = self.root.join(&name);
        if tokio::fs::try_exists(&path)
            .await
            .map_err(|_| EmailAttachmentError::Unavailable)?
        {
            return Ok(name);
        }
        // The oldest give way rather than the newest being turned away. A cache
        // that refuses once full stops working the moment it is used.
        let fits = crate::private_store::make_room_for(
            self.root.as_ref(),
            bytes.len() as u64,
            MAX_EMAIL_ATTACHMENT_FILES,
            EMAIL_ATTACHMENT_DISK_PERCENT,
        )
        .await
        .map_err(|_| EmailAttachmentError::Unavailable)?;
        if !fits {
            return Err(EmailAttachmentError::Capacity);
        }
        write_private(&path, bytes).await?;
        let metadata_path = self.root.join(format!("{name}.type"));
        if let Err(error) = write_private(&metadata_path, normalized_type.as_bytes()).await {
            let _ = tokio::fs::remove_file(&path).await;
            return Err(error);
        }
        Ok(name)
    }

    pub(crate) async fn read(&self, name: &str) -> Result<(Vec<u8>, String), EmailAttachmentError> {
        validate_storage_name(name)?;
        let path = self.root.join(name);
        let metadata = tokio::fs::symlink_metadata(&path)
            .await
            .map_err(|error| map_not_found(&error))?;
        if !metadata.file_type().is_file()
            || metadata.len() == 0
            || metadata.len() > MAX_EMAIL_ATTACHMENT_BYTES as u64
        {
            return Err(EmailAttachmentError::NotFound);
        }
        let bytes = tokio::fs::read(&path)
            .await
            .map_err(|_| EmailAttachmentError::Unavailable)?;
        let media_type = tokio::fs::read_to_string(self.root.join(format!("{name}.type")))
            .await
            .map_err(|error| map_not_found(&error))?;
        attachment_kind(media_type.trim(), &bytes)?;
        Ok((bytes, media_type.trim().to_owned()))
    }
}

fn attachment_kind<'a>(
    media_type: &'a str,
    bytes: &[u8],
) -> Result<(&'static str, &'a str), EmailAttachmentError> {
    let normalized = media_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    let extension = match normalized.as_str() {
        "image/png" if bytes.starts_with(b"\x89PNG\r\n\x1a\n") => "png",
        "image/jpeg" if bytes.starts_with(&[0xff, 0xd8, 0xff]) => "jpg",
        "image/gif" if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") => "gif",
        "image/webp" if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" => {
            "webp"
        }
        "application/pdf" if bytes.starts_with(b"%PDF-") => "pdf",
        "application/zip"
        | "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        | "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
        | "application/vnd.openxmlformats-officedocument.presentationml.presentation"
            if bytes.starts_with(b"PK") =>
        {
            "zip"
        }
        "text/plain" => "txt",
        "text/csv" => "csv",
        "application/json" if serde_json::from_slice::<serde_json::Value>(bytes).is_ok() => "json",
        "message/rfc822" => "eml",
        "image/png"
        | "image/jpeg"
        | "image/gif"
        | "image/webp"
        | "application/pdf"
        | "application/zip"
        | "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        | "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
        | "application/vnd.openxmlformats-officedocument.presentationml.presentation"
        | "application/json" => return Err(EmailAttachmentError::InvalidSignature),
        _ => "bin",
    };
    Ok((
        extension,
        media_type.split(';').next().unwrap_or_default().trim(),
    ))
}

fn validate_storage_name(name: &str) -> Result<(), EmailAttachmentError> {
    let Some((digest, extension)) = name.split_once('.') else {
        return Err(EmailAttachmentError::NotFound);
    };
    if digest.len() != 32
        || !digest.bytes().all(|byte| byte.is_ascii_hexdigit())
        || extension.is_empty()
        || extension.len() > 8
        || !extension.bytes().all(|byte| byte.is_ascii_alphanumeric())
    {
        return Err(EmailAttachmentError::NotFound);
    }
    Ok(())
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
async fn write_private(path: &Path, bytes: &[u8]) -> Result<(), EmailAttachmentError> {
    #[cfg(unix)]
    {
        use tokio::io::AsyncWriteExt;
        let mut options = tokio::fs::OpenOptions::new();
        options.write(true).create_new(true).mode(0o600);
        let mut file = options
            .open(path)
            .await
            .map_err(|_| EmailAttachmentError::Unavailable)?;
        if file.write_all(bytes).await.is_err() || file.sync_all().await.is_err() {
            drop(file);
            let _ = tokio::fs::remove_file(path).await;
            return Err(EmailAttachmentError::Unavailable);
        }
        Ok(())
    }
    #[cfg(not(unix))]
    tokio::fs::write(path, bytes)
        .await
        .map_err(|_| EmailAttachmentError::Unavailable)
}

fn secure_directory(path: &Path) -> Result<(), EmailAttachmentError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .map_err(|_| EmailAttachmentError::Unavailable)?;
    }
    Ok(())
}

fn map_not_found(error: &std::io::Error) -> EmailAttachmentError {
    if error.kind() == std::io::ErrorKind::NotFound {
        EmailAttachmentError::NotFound
    } else {
        EmailAttachmentError::Unavailable
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn stores_images_documents_and_opaque_files_privately() {
        let directory = tempfile::tempdir().unwrap();
        let store = EmailAttachmentStore::new(directory.path().to_path_buf());
        for (media_type, bytes) in [
            ("image/png", b"\x89PNG\r\n\x1a\nimage".as_slice()),
            ("application/pdf", b"%PDF-private".as_slice()),
            ("application/octet-stream", b"opaque".as_slice()),
        ] {
            let name = store.save(media_type, bytes).await.unwrap();
            assert!(!name.contains("private"));
            let (loaded, loaded_type) = store.read(&name).await.unwrap();
            assert_eq!(loaded, bytes);
            assert_eq!(loaded_type, media_type);
        }
    }

    #[tokio::test]
    async fn rejects_spoofed_content_and_traversal() {
        let directory = tempfile::tempdir().unwrap();
        let store = EmailAttachmentStore::new(directory.path().to_path_buf());
        assert!(matches!(
            store.save("image/png", b"not png").await,
            Err(EmailAttachmentError::InvalidSignature)
        ));
        assert!(matches!(
            store.read("../private.bin").await,
            Err(EmailAttachmentError::NotFound)
        ));
    }
}

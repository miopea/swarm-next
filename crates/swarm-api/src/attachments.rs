use std::{
    fmt::Write,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime},
};

use sha2::{Digest, Sha256};
use tokio::sync::Mutex;

pub const MAX_ATTACHMENT_BYTES: usize = 8 * 1024 * 1024;
/// The store's share of whatever disk it lives on, rather than a fixed number.
///
/// A flat 128 MiB is nothing when screenshots arrive all day, and it is also
/// the wrong answer in the other direction — on a nearly full disk a fixed
/// allowance keeps claiming space that is no longer there. A share of what is
/// actually free is self-correcting: as the disk fills the allowance shrinks
/// and eviction gives the space back.
const ATTACHMENT_STORE_DISK_PERCENT: u64 = 5;
/// A directory-size sanity bound, not the real limit. Bytes govern: with a
/// per-file ceiling of 8 MiB against a 128 MiB store, a file count of 64 made
/// the store refuse writes at 4.5% of its byte budget — which is exactly what
/// happened, after two days of ordinary use.
const MAX_ATTACHMENT_FILES: usize = 512;
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
    #[error("private attachment was not found")]
    NotFound,
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
        let fits = crate::private_store::make_room_for(
            self.root.as_ref(),
            bytes.len() as u64,
            MAX_ATTACHMENT_FILES,
            ATTACHMENT_STORE_DISK_PERCENT,
        )
        .await
        .map_err(|_| AttachmentError::Unavailable)?;
        if !fits {
            return Err(AttachmentError::Capacity);
        }
        write_private(&path, bytes).await?;
        Ok(path)
    }

    pub async fn read(&self, name: &str) -> Result<(Vec<u8>, &'static str), AttachmentError> {
        let media_type = media_type_for_name(name).ok_or(AttachmentError::NotFound)?;
        let path = self.root.join(name);
        let metadata = tokio::fs::symlink_metadata(&path).await.map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                AttachmentError::NotFound
            } else {
                AttachmentError::Unavailable
            }
        })?;
        if !metadata.file_type().is_file()
            || metadata.len() == 0
            || metadata.len() > MAX_ATTACHMENT_BYTES as u64
        {
            return Err(AttachmentError::NotFound);
        }
        let bytes = tokio::fs::read(path)
            .await
            .map_err(|_| AttachmentError::Unavailable)?;
        validated_extension(media_type, &bytes).map_err(|_| AttachmentError::NotFound)?;
        Ok((bytes, media_type))
    }
}

fn media_type_for_name(name: &str) -> Option<&'static str> {
    let (digest, extension) = name.split_once('.')?;
    if digest.len() != 20 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    match extension {
        "png" => Some("image/png"),
        "jpg" => Some("image/jpeg"),
        "webp" => Some("image/webp"),
        "gif" => Some("image/gif"),
        _ => None,
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

    #[tokio::test]
    async fn reads_only_valid_content_named_images() {
        let directory = tempfile::tempdir().unwrap();
        let store = AttachmentStore::new(directory.path().join("attachments"));
        let path = store.save("image/png", PNG).await.unwrap();
        let name = path.file_name().unwrap().to_string_lossy();

        let (bytes, media_type) = store.read(&name).await.unwrap();

        assert_eq!(bytes, PNG);
        assert_eq!(media_type, "image/png");
        assert!(matches!(
            store.read("../private.png").await,
            Err(AttachmentError::NotFound)
        ));
        assert!(matches!(
            store.read("00000000000000000000.exe").await,
            Err(AttachmentError::NotFound)
        ));
        assert!(matches!(
            store.read("00000000000000000000.png").await,
            Err(AttachmentError::NotFound)
        ));
    }

    /// The operator's report, reproduced exactly: "Image could not be added —
    /// Runtime request returned 507: private attachment storage is full."
    ///
    /// The live store held 64 files — precisely the old file cap — totalling
    /// 5.8 MB against a 128 MB byte budget. So it refused writes at 4.5% of
    /// its capacity, and the seven-day expiry could not help because the oldest
    /// file was two days old. A private cache that refuses new writes once full
    /// stops working the moment it is used.
    ///
    /// It also explains the earlier intermittent report — "it worked when I
    /// tried it a second time". A repeat of the same image is content-addressed
    /// and returns the existing file without needing room; only a new image
    /// failed.
    #[tokio::test]
    async fn a_full_store_makes_room_instead_of_refusing_the_paste() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("attachments");
        let store = AttachmentStore::new(root.clone());

        // Fill it past the file cap, well under the byte budget, as the live
        // store was.
        let mut first_saved = None;
        for index in 0..MAX_ATTACHMENT_FILES {
            let image = [PNG, format!("-{index}").as_bytes()].concat();
            let path = store.save("image/png", &image).await.unwrap();
            if index == 0 {
                first_saved = Some(path);
            }
        }
        let (files, bytes) = crate::private_store::usage(&root).await.unwrap();
        assert_eq!(files, MAX_ATTACHMENT_FILES);
        assert!(bytes < 64 * 1024 * 1024, "full by count, not by size");

        // The next paste succeeds rather than being refused.
        let arriving = [PNG, b"-the-one-the-operator-just-pasted"].concat();
        let saved = store.save("image/png", &arriving).await.unwrap();
        assert_eq!(tokio::fs::read(&saved).await.unwrap(), arriving);

        // Room came from the oldest, and the store stayed within its bound.
        assert!(!tokio::fs::try_exists(first_saved.unwrap()).await.unwrap());
        assert!(crate::private_store::usage(&root).await.unwrap().0 <= MAX_ATTACHMENT_FILES);
    }

    /// Pasting the same image twice never needs room: it is content-addressed.
    #[tokio::test]
    async fn the_same_image_twice_is_stored_once() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("attachments");
        let store = AttachmentStore::new(root.clone());

        let first = store.save("image/png", PNG).await.unwrap();
        let second = store.save("image/png", PNG).await.unwrap();

        assert_eq!(first, second);
        assert_eq!(crate::private_store::usage(&root).await.unwrap().0, 1);
    }

    /// "The allowance should be a % of disk space, not 128. That is nothing
    /// with images coming in all the time especially on a large drive."
    ///
    /// A share of what is free is also the right answer as a disk fills: the
    /// allowance shrinks with it, and eviction hands the space back.
    /// This store is bounded by the shared rules rather than by numbers of its
    /// own: a share of the disk beneath it, and eviction rather than refusal.
    /// The rules themselves are proved in `private_store`; what matters here is
    /// that this store is subject to them.
    #[tokio::test]
    async fn the_private_store_is_bounded_by_the_shared_rules() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("attachments");
        let store = AttachmentStore::new(root.clone());
        store.save("image/png", PNG).await.unwrap();

        let allowance = crate::private_store::store_allowance(&root, ATTACHMENT_STORE_DISK_PERCENT);

        // Never worse than the fixed allowance it replaced, and never a reason
        // a disk filled.
        assert!(allowance >= 128 * 1024 * 1024);
        assert!(allowance <= 8 * 1024 * 1024 * 1024);
    }

    /// A store already inside its bound throws nothing away.
    #[tokio::test]
    async fn a_store_within_its_bound_keeps_what_it_has() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("attachments");
        let store = AttachmentStore::new(root.clone());
        let kept = store.save("image/png", PNG).await.unwrap();

        store
            .save("image/png", &[PNG, b"-second"].concat())
            .await
            .unwrap();

        assert!(tokio::fs::try_exists(&kept).await.unwrap());
    }
}

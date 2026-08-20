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
/// Never smaller than the fixed allowance it replaces, and never large enough
/// to be the reason a disk filled.
const MIN_ATTACHMENT_STORE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_ATTACHMENT_STORE_BYTES: u64 = 8 * 1024 * 1024 * 1024;
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
        make_room_for(self.root.as_ref(), bytes.len() as u64).await?;
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

/// How much this store may hold right now, from the free space beneath it.
///
/// Re-read on every write, so the answer tracks the disk rather than a number
/// chosen once. When the filesystem cannot be questioned the floor applies,
/// which is the allowance this replaced.
fn store_allowance(root: &Path) -> u64 {
    let available = nix::sys::statvfs::statvfs(root).ok().map(|stats| {
        stats
            .blocks_available()
            .saturating_mul(stats.fragment_size())
    });
    let Some(available) = available else {
        return MIN_ATTACHMENT_STORE_BYTES;
    };
    available
        .saturating_mul(ATTACHMENT_STORE_DISK_PERCENT)
        .saturating_div(100)
        .clamp(MIN_ATTACHMENT_STORE_BYTES, MAX_ATTACHMENT_STORE_BYTES)
}

/// Evicts the oldest attachments until the incoming one fits.
///
/// A bounded private cache that refuses new writes when it is full is a cache
/// that stops working the moment it is used — the operator hit exactly that,
/// pasting a screenshot into a full store and being told to try again, which
/// could never succeed. Age-based pruning does not help either: a store can
/// fill in two days while the expiry is seven.
///
/// Oldest-first, because these are screenshots attached to messages that have
/// already been sent and the newest are the ones still being discussed. Eviction
/// can break an old image link, which the seven-day expiry already does; the
/// difference is that this happens when the space is needed rather than on a
/// calendar.
async fn make_room_for(root: &Path, incoming: u64) -> Result<(), AttachmentError> {
    let allowance = store_allowance(root);
    let (mut files, mut retained) = store_usage(root).await?;
    if files < MAX_ATTACHMENT_FILES && retained.saturating_add(incoming) <= allowance {
        return Ok(());
    }
    let mut oldest_first = attachments_by_age(root).await?;
    while (files >= MAX_ATTACHMENT_FILES || retained.saturating_add(incoming) > allowance)
        && !oldest_first.is_empty()
    {
        let (_, path, size) = oldest_first.remove(0);
        // A file already gone is the outcome wanted here, so its absence counts
        // the same as removing it.
        let _ = tokio::fs::remove_file(&path).await;
        files = files.saturating_sub(1);
        retained = retained.saturating_sub(size);
    }
    // Only a file larger than the whole allowance can still not fit, and the
    // per-file ceiling is a fraction of the floor.
    if retained.saturating_add(incoming) > allowance {
        return Err(AttachmentError::Capacity);
    }
    Ok(())
}

/// Every stored attachment with its age and size, oldest first.
async fn attachments_by_age(
    root: &Path,
) -> Result<Vec<(SystemTime, std::path::PathBuf, u64)>, AttachmentError> {
    let mut entries = tokio::fs::read_dir(root)
        .await
        .map_err(|_| AttachmentError::Unavailable)?;
    let mut found = Vec::new();
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
            found.push((
                metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                entry.path(),
                metadata.len(),
            ));
        }
    }
    found.sort_by_key(|(modified, _, _)| *modified);
    Ok(found)
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
        let (files, bytes) = store_usage(&root).await.unwrap();
        assert_eq!(files, MAX_ATTACHMENT_FILES);
        assert!(
            bytes < MIN_ATTACHMENT_STORE_BYTES / 2,
            "full by count, not by size"
        );

        // The next paste succeeds rather than being refused.
        let arriving = [PNG, b"-the-one-the-operator-just-pasted"].concat();
        let saved = store.save("image/png", &arriving).await.unwrap();
        assert_eq!(tokio::fs::read(&saved).await.unwrap(), arriving);

        // Room came from the oldest, and the store stayed within its bound.
        assert!(!tokio::fs::try_exists(first_saved.unwrap()).await.unwrap());
        assert!(store_usage(&root).await.unwrap().0 <= MAX_ATTACHMENT_FILES);
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
        assert_eq!(store_usage(&root).await.unwrap().0, 1);
    }

    /// "The allowance should be a % of disk space, not 128. That is nothing
    /// with images coming in all the time especially on a large drive."
    ///
    /// A share of what is free is also the right answer as a disk fills: the
    /// allowance shrinks with it, and eviction hands the space back.
    #[tokio::test]
    async fn the_allowance_follows_the_disk_and_never_drops_below_the_old_one() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("attachments");
        tokio::fs::create_dir_all(&root).await.unwrap();

        let allowance = store_allowance(&root);

        assert!(
            allowance >= MIN_ATTACHMENT_STORE_BYTES,
            "never worse than the fixed allowance it replaced"
        );
        assert!(
            allowance <= MAX_ATTACHMENT_STORE_BYTES,
            "never large enough to be why a disk filled"
        );
    }

    /// A filesystem that cannot be questioned still gets a usable answer.
    #[test]
    fn an_unreadable_filesystem_falls_back_to_the_floor() {
        assert_eq!(
            store_allowance(Path::new("/definitely/not/a/real/mount/point")),
            MIN_ATTACHMENT_STORE_BYTES
        );
    }
}

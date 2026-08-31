use std::{
    fmt::Write,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime},
};

use sha2::{Digest, Sha256};
use tokio::sync::Mutex;

/// The largest image a terminal will take.
///
/// Raised from 8 MiB on the operator's instruction, 2026-08-24: "I cannot
/// shrink it, we need a reasonable limit." Eight was comfortable for a
/// screenshot and too tight for a screen recording, which is what people
/// actually reach for when showing a bug that moves.
///
/// The cost of the larger number is disk and one upload's worth of memory. It
/// does not reach a provider's context: the attachment is written to the
/// terminal as a path, so what a provider spends is unchanged.
pub const MAX_ATTACHMENT_BYTES: usize = 32 * 1024 * 1024;
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
    #[error("file content did not match its declared type")]
    InvalidSignature,
    #[error("file must contain 1 to {MAX_ATTACHMENT_BYTES} bytes")]
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
        "csv" => Some("text/csv"),
        "md" => Some("text/markdown"),
        "txt" => Some("text/plain"),
        "json" => Some("application/json"),
        "pdf" => Some("application/pdf"),
        "zip" => Some("application/zip"),
        "xlsx" => Some(XLSX),
        "docx" => Some(DOCX),
        "pptx" => Some(PPTX),
        "xls" => Some(XLS),
        "mp4" => Some("video/mp4"),
        "gz" => Some("application/gzip"),
        "tar" => Some("application/x-tar"),
        "bin" => Some("application/octet-stream"),
        _ => None,
    }
}

/// The Open XML media type for a modern Excel workbook.
pub const XLSX: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";
/// The Open XML media type for a Word document.
pub const DOCX: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.document";
/// The Open XML media type for a `PowerPoint` deck.
pub const PPTX: &str = "application/vnd.openxmlformats-officedocument.presentationml.presentation";
/// The media type for a legacy Excel workbook.
pub const XLS: &str = "application/vnd.ms-excel";

/// The extension to store a media type under, once its bytes agree with it.
///
/// Mirrors `attachment_kind` in `email_attachments.rs` deliberately: this Hive
/// already decided how to store an arbitrary file safely and there is no reason
/// for a terminal drop to decide differently.
///
/// The rule is that a file which DECLARES a format with a signature must match
/// it — claiming image/png and not being a PNG is a mislabel worth refusing.
/// Everything else is stored opaquely as .bin.
///
/// Text is deliberately NOT validated. An earlier version required UTF-8 and no
/// NUL for text/csv, which was wrong twice over. It rejected real spreadsheets:
/// Excel exports CSV as Windows-1252 often enough, and its "Unicode text" export
/// is UTF-16, which is full of NULs. And it bought nothing, because once unknown
/// types fall through to .bin the same rejected bytes are accepted by calling
/// them application/octet-stream. A guard that can be sidestepped by renaming
/// the type is not a guard.
///
/// XLSX, DOCX and PPTX are ZIP containers, so the PK check proves the file is a
/// zip and not that it is a workbook. That is the strongest check the format
/// allows and it is not described as more.
fn validated_extension(media_type: &str, bytes: &[u8]) -> Result<&'static str, AttachmentError> {
    const OLE2: [u8; 8] = [0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1];
    const OPEN_XML: [&str; 3] = [XLSX, DOCX, PPTX];
    let normalized = media_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    let signed_but_wrong = || Err(AttachmentError::InvalidSignature);
    Ok(match normalized.as_str() {
        "image/png" if bytes.starts_with(b"\x89PNG\r\n\x1a\n") => "png",
        "image/jpeg" if bytes.starts_with(&[0xff, 0xd8, 0xff]) => "jpg",
        "image/gif" if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") => "gif",
        "image/webp" if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" => {
            "webp"
        }
        "application/pdf" if bytes.starts_with(b"%PDF-") => "pdf",
        "application/zip" if bytes.starts_with(b"PK") => "zip",
        // EACH OOXML TYPE KEEPS ITS OWN NAME. All three used to return "xlsx",
        // so a Word document and a PowerPoint deck arrived claiming to be a
        // spreadsheet. That is worse than the .bin fallback: .bin says nothing,
        // .xlsx says something false, and anything dispatching on extension
        // opens it wrong and fails as though the file were corrupt.
        //
        // The PK check still only proves the container is a zip, exactly as the
        // comment above says. What changed is that the RESULT no longer names a
        // workbook regardless of what was declared.
        XLSX if bytes.starts_with(b"PK") => "xlsx",
        DOCX if bytes.starts_with(b"PK") => "docx",
        PPTX if bytes.starts_with(b"PK") => "pptx",
        XLS if bytes.starts_with(&OLE2) => "xls",
        // ARCHIVES AND MEDIA, so what arrives can be told apart from anything
        // else. These were reaching a worker as `<digest>.bin` — no name, no
        // extension, no hint — which is indistinguishable from the drop having
        // failed. The operator reported exactly that about an mp4.
        //
        // Naming them does not make a worker able to READ them: an agent still
        // cannot process video. It makes the file identifiable instead of
        // anonymous, which is the difference between a failure someone can
        // reason about and one that looks like a broken feature.
        "video/mp4" if bytes.len() >= 8 && &bytes[4..8] == b"ftyp" => "mp4",
        "application/gzip" | "application/x-gzip" if bytes.starts_with(&[0x1f, 0x8b]) => "gz",
        // `ustar` sits at offset 257 of the first header block, which is why a
        // tar is not recognisable from its first bytes like everything else.
        "application/x-tar" if bytes.len() >= 262 && &bytes[257..262] == b"ustar" => "tar",
        "text/csv" => "csv",
        "text/markdown" => "md",
        "text/plain" => "txt",
        "application/json" => "json",
        "image/png" | "image/jpeg" | "image/gif" | "image/webp" | "application/pdf"
        | "application/zip" | XLS | "video/mp4" | "application/gzip" | "application/x-gzip"
        | "application/x-tar" => return signed_but_wrong(),
        value if OPEN_XML.contains(&value) => return signed_but_wrong(),
        _ => "bin",
    })
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

    /// Most file types are storable, and a declared format still has to match.
    ///
    /// The contract has two halves and the second is the one worth guarding. A
    /// file that declares a format WITH a signature must match it. A file that
    /// declares anything else is stored opaquely as .bin rather than refused,
    /// because the operator asked to be able to drop most file types and an
    /// allow-list cannot anticipate them.
    ///
    /// Text is not validated, and the assertion that a binary wearing text/csv
    /// is ACCEPTED is deliberate rather than an oversight. Refusing it bought
    /// nothing once .bin exists — the identical bytes are storable by calling
    /// them octet-stream — and the check it replaced rejected real Excel CSV
    /// exports, which are frequently Windows-1252 and sometimes UTF-16.
    #[tokio::test]
    async fn stores_most_types_and_still_refuses_a_mislabelled_format() {
        let directory = tempfile::tempdir().unwrap();
        let store = AttachmentStore::new(directory.path().join("attachments"));
        let extension_of = |path: std::path::PathBuf| {
            path.extension()
                .and_then(|value| value.to_str())
                .unwrap()
                .to_string()
        };

        let csv = store
            .save("text/csv", b"name,amount\nvicky,3\n")
            .await
            .unwrap();
        assert_eq!(extension_of(csv.clone()), "csv");
        assert_eq!(
            extension_of(store.save(XLSX, b"PK\x03\x04workbook").await.unwrap()),
            "xlsx"
        );
        // THIS LINE USED TO EXPECT "xlsx", which is why the defect was invisible:
        // the behaviour was wrong and the test agreed with it, so nothing could
        // ever report it. A Word document stored as a spreadsheet fails later
        // as a corrupt file rather than a mislabelled one.
        assert_eq!(
            extension_of(store.save(DOCX, b"PK\x03\x04document").await.unwrap()),
            "docx"
        );
        assert_eq!(
            extension_of(store.save(PPTX, b"PK\x03\x04deck").await.unwrap()),
            "pptx"
        );
        assert_eq!(
            extension_of(
                store
                    .save(XLS, b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1legacy")
                    .await
                    .unwrap()
            ),
            "xls"
        );
        assert_eq!(
            extension_of(
                store
                    .save("application/pdf", b"%PDF-1.7 body")
                    .await
                    .unwrap()
            ),
            "pdf"
        );

        // Anything unrecognised is stored rather than refused. This is the half
        // the operator asked for: "we should definitely be able to drag and drop
        // most file types."
        assert_eq!(
            extension_of(
                store
                    .save("application/octet-stream", b"\x00\x01opaque")
                    .await
                    .unwrap()
            ),
            "bin"
        );
        assert_eq!(
            extension_of(
                store
                    .save("application/x-sqlite3", b"SQLite format 3\x00")
                    .await
                    .unwrap()
            ),
            "bin"
        );
        assert_eq!(
            extension_of(
                store
                    .save("text/csv", b"\x00\x01not really text")
                    .await
                    .unwrap()
            ),
            "csv"
        );

        // The half that still bites: claiming a signed format you are not.
        // Without these arms every one of these would be stored as .bin.
        for (media_type, bytes) in [
            ("image/png", b"not a png".as_slice()),
            ("application/pdf", b"not a pdf".as_slice()),
            (XLSX, b"not a zip".as_slice()),
            (XLS, b"not an ole2 file".as_slice()),
        ] {
            assert!(
                matches!(
                    store.save(media_type, bytes).await,
                    Err(AttachmentError::InvalidSignature)
                ),
                "{media_type} should refuse content that is not that format"
            );
        }

        // Empty is refused one layer earlier, by the size guard in save.
        assert!(matches!(
            store.save("text/csv", b"").await,
            Err(AttachmentError::InvalidSize)
        ));

        // A stored file must read back through the same validation, or it would
        // save and then 404 on retrieval.
        let name = csv.file_name().unwrap().to_string_lossy().to_string();
        let (bytes, media_type) = store.read(&name).await.unwrap();
        assert_eq!(media_type, "text/csv");
        assert_eq!(bytes, b"name,amount\nvicky,3\n");
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

    /// THE ARM THAT SAID SOMETHING FALSE. All three OOXML types returned
    /// `xlsx`, so a Word document and a `PowerPoint` deck arrived claiming to
    /// be a spreadsheet — worse than the `bin` fallback, because `bin` says
    /// nothing and `xlsx` says something untrue.
    #[tokio::test]
    async fn a_word_document_and_a_deck_are_not_stored_as_spreadsheets() {
        let directory = tempfile::tempdir().unwrap();
        let store = AttachmentStore::new(directory.path().join("attachments"));
        // Any zip: the PK check only ever proved the container, which is what
        // the comment on validated_extension has always said.
        let container = b"PK\x03\x04 pretend this is an office document";

        let word = store.save(DOCX, container).await.unwrap();
        let deck = store.save(PPTX, container).await.unwrap();
        let sheet = store.save(XLSX, container).await.unwrap();

        assert!(word.to_string_lossy().ends_with(".docx"));
        assert!(deck.to_string_lossy().ends_with(".pptx"));
        assert!(sheet.to_string_lossy().ends_with(".xlsx"));
    }

    /// The operator: "doesn't seem I can send a mp4 via drag/drop", and later
    /// "I need to be able to upload the mp4 or other binaries like zip/tar".
    ///
    /// The upload always worked. What arrived was `<digest>.bin` — no name, no
    /// extension, nothing to tell it from any other opaque blob — which is
    /// indistinguishable from the drop having failed.
    #[tokio::test]
    async fn an_archive_or_a_video_arrives_named_as_what_it_is() {
        let directory = tempfile::tempdir().unwrap();
        let store = AttachmentStore::new(directory.path().join("attachments"));

        let mut mp4 = vec![0u8; 4];
        mp4.extend_from_slice(b"ftypisom");
        let video = store.save("video/mp4", &mp4).await.unwrap();
        assert!(video.to_string_lossy().ends_with(".mp4"));

        let gzip = store
            .save("application/gzip", &[0x1f, 0x8b, 0x08, 0x00, 0x00])
            .await
            .unwrap();
        assert!(gzip.to_string_lossy().ends_with(".gz"));

        // `ustar` lives at offset 257, not in the first bytes.
        let mut tar = vec![0u8; 257];
        tar.extend_from_slice(b"ustar\0");
        let archive = store.save("application/x-tar", &tar).await.unwrap();
        assert!(archive.to_string_lossy().ends_with(".tar"));

        let zip = store
            .save("application/zip", b"PK\x03\x04zip")
            .await
            .unwrap();
        assert!(zip.to_string_lossy().ends_with(".zip"));
    }

    /// EVERY NAME THIS STORE WRITES MUST READ BACK. `read` resolves a media
    /// type from the filename and refuses one it does not know, so adding an
    /// extension to the write path without the read path stores files that can
    /// never be fetched — a silent one-way door.
    #[tokio::test]
    async fn every_extension_the_store_writes_can_be_read_back() {
        let directory = tempfile::tempdir().unwrap();
        let store = AttachmentStore::new(directory.path().join("attachments"));
        let mut mp4 = vec![0u8; 4];
        mp4.extend_from_slice(b"ftypisom");
        let mut tar = vec![0u8; 257];
        tar.extend_from_slice(b"ustar\0");

        let written: Vec<(&str, Vec<u8>)> = vec![
            ("image/png", PNG.to_vec()),
            ("application/pdf", b"%PDF-1.7 body".to_vec()),
            ("application/zip", b"PK\x03\x04zip".to_vec()),
            (XLSX, b"PK\x03\x04sheet".to_vec()),
            (DOCX, b"PK\x03\x04word".to_vec()),
            (PPTX, b"PK\x03\x04deck".to_vec()),
            ("text/plain", b"notes".to_vec()),
            ("application/json", b"{}".to_vec()),
            ("video/mp4", mp4),
            ("application/gzip", vec![0x1f, 0x8b, 0x08, 0x00, 0x00]),
            ("application/x-tar", tar),
            // The opaque fallback still round-trips.
            ("application/octet-stream", b"anything at all".to_vec()),
        ];

        for (media_type, bytes) in written {
            let path = store.save(media_type, &bytes).await.unwrap();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let (read_back, _) = store.read(&name).await.unwrap_or_else(|_| {
                panic!("{media_type} was stored as {name} and could not be read back")
            });
            assert_eq!(
                read_back, bytes,
                "{media_type} round-tripped different bytes"
            );
        }
    }

    /// A file that DECLARES a format and does not match it is still refused.
    /// The new types must not become a hole in that rule.
    #[tokio::test]
    async fn a_declared_format_that_does_not_match_its_bytes_is_still_refused() {
        let directory = tempfile::tempdir().unwrap();
        let store = AttachmentStore::new(directory.path().join("attachments"));
        for media_type in ["video/mp4", "application/gzip", "application/x-tar"] {
            assert!(
                store
                    .save(media_type, b"not that format at all")
                    .await
                    .is_err(),
                "{media_type} accepted bytes that are not one"
            );
        }
    }
}

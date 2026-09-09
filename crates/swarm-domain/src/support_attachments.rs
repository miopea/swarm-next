//! Immutable reviewed files for native feedback. No paths, URLs or read authority.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::HashSet, fmt};
use uuid::Uuid;

pub const SUPPORT_ATTACHMENT_MAX_COUNT: usize = 4;
pub const SUPPORT_ATTACHMENT_MAX_BYTES: usize = 5 * 1024 * 1024;
pub const SUPPORT_ATTACHMENTS_MAX_BYTES: usize = 12 * 1024 * 1024;

/// Private reviewed metadata: deliberately not Debug, as filenames may be sensitive.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupportAttachmentMetadata {
    pub id: Uuid,
    pub file_name: String,
    pub media_type: String,
    pub size_bytes: usize,
    pub sha256: String,
}

/// Only validation constructs a file; callers cannot replace bytes after review.
#[derive(Clone, PartialEq, Eq)]
pub struct SupportAttachment {
    metadata: SupportAttachmentMetadata,
    bytes: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SupportAttachmentError {
    Identity,
    FileName,
    MediaType,
    Size,
    Digest,
    Text,
    Count,
}
impl fmt::Display for SupportAttachmentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Identity => "attachment identities must be nonnil and unique",
            Self::FileName => {
                "attachment filename must be bounded and contain no path or control characters"
            }
            Self::MediaType => "only PNG, JPEG, WebP and plain text attachments are supported",
            Self::Size => "attachment byte count or combined size exceeds the allowed bounds",
            Self::Digest => "attachment bytes do not match their reviewed SHA256 digest",
            Self::Text => "plain text attachments must be UTF-8 without NUL",
            Self::Count => "a feedback report supports at most four attachments",
        })
    }
}
impl std::error::Error for SupportAttachmentError {}

impl SupportAttachment {
    /// Freezes verified bytes. Admin additionally verifies raster decoding/animation
    /// before accepting the public upload; this does not claim remote acceptance.
    ///
    /// # Errors
    /// Rejects unsupported metadata, changed bytes and oversized content.
    pub fn validate(
        metadata: SupportAttachmentMetadata,
        bytes: Vec<u8>,
    ) -> Result<Self, SupportAttachmentError> {
        if metadata.id.is_nil() {
            return Err(SupportAttachmentError::Identity);
        }
        if metadata.file_name.is_empty()
            || metadata.file_name.encode_utf16().count() > 180
            || metadata
                .file_name
                .chars()
                .any(|c| c.is_control() || matches!(c, '/' | '\\'))
        {
            return Err(SupportAttachmentError::FileName);
        }
        if !matches!(
            metadata.media_type.as_str(),
            "image/png" | "image/jpeg" | "image/webp" | "text/plain"
        ) {
            return Err(SupportAttachmentError::MediaType);
        }
        if bytes.len() != metadata.size_bytes || bytes.len() > SUPPORT_ATTACHMENT_MAX_BYTES {
            return Err(SupportAttachmentError::Size);
        }
        if metadata.sha256.len() != 64 || metadata.sha256 != format!("{:x}", Sha256::digest(&bytes))
        {
            return Err(SupportAttachmentError::Digest);
        }
        if metadata.media_type == "text/plain"
            && (bytes.contains(&0) || std::str::from_utf8(&bytes).is_err())
        {
            return Err(SupportAttachmentError::Text);
        }
        Ok(Self { metadata, bytes })
    }

    #[must_use]
    pub const fn metadata(&self) -> &SupportAttachmentMetadata {
        &self.metadata
    }
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// Empty means the existing text-only contract; an attachment route must require
/// at least one file separately. Order is preserved as part of reviewed identity.
///
/// # Errors
/// Rejects duplicate identities, excessive count and combined byte capacity.
pub fn validate_support_attachment_set(
    files: &[SupportAttachment],
) -> Result<(), SupportAttachmentError> {
    if files.len() > SUPPORT_ATTACHMENT_MAX_COUNT {
        return Err(SupportAttachmentError::Count);
    }
    let mut ids = HashSet::new();
    let mut bytes = 0usize;
    for file in files {
        if !ids.insert(file.metadata.id) {
            return Err(SupportAttachmentError::Identity);
        }
        bytes = bytes.saturating_add(file.bytes.len());
    }
    if bytes > SUPPORT_ATTACHMENTS_MAX_BYTES {
        return Err(SupportAttachmentError::Size);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn metadata(bytes: &[u8]) -> SupportAttachmentMetadata {
        SupportAttachmentMetadata {
            id: Uuid::from_u128(1),
            file_name: "fictional.txt".into(),
            media_type: "text/plain".into(),
            size_bytes: bytes.len(),
            sha256: format!("{:x}", Sha256::digest(bytes)),
        }
    }
    #[test]
    fn validates_exact_bytes_and_rejects_changed_hash_size_identity_type_and_name() {
        let bytes = b"fictional";
        assert!(SupportAttachment::validate(metadata(bytes), bytes.to_vec()).is_ok());
        for (field, value) in [
            ("file_name", "../secret"),
            ("file_name", "a\\b"),
            ("file_name", "a\nb"),
            ("media_type", "image/svg+xml"),
            ("sha256", "wrong"),
            ("id", "00000000-0000-0000-0000-000000000000"),
        ] {
            let mut encoded = serde_json::to_value(metadata(bytes)).unwrap();
            encoded[field] = value.into();
            assert!(
                SupportAttachment::validate(
                    serde_json::from_value(encoded).unwrap(),
                    bytes.to_vec()
                )
                .is_err()
            );
        }
        assert!(SupportAttachment::validate(metadata(bytes), b"different".to_vec()).is_err());
        for invalid in [vec![0], vec![255]] {
            assert!(matches!(
                SupportAttachment::validate(metadata(&invalid), invalid),
                Err(SupportAttachmentError::Text)
            ));
        }
    }
    #[test]
    fn set_is_bounded_and_cannot_repeat_a_file_identity() {
        let file = SupportAttachment::validate(metadata(b"a"), b"a".to_vec()).unwrap();
        assert!(matches!(
            validate_support_attachment_set(&[file.clone(), file.clone()]),
            Err(SupportAttachmentError::Identity)
        ));
        assert!(matches!(
            validate_support_attachment_set(&vec![file; 5]),
            Err(SupportAttachmentError::Count)
        ));
        let files = (1..=3)
            .map(|id| {
                let bytes = vec![b'a'; SUPPORT_ATTACHMENT_MAX_BYTES];
                let mut meta = metadata(&bytes);
                meta.id = Uuid::from_u128(id);
                SupportAttachment::validate(meta, bytes).unwrap()
            })
            .collect::<Vec<_>>();
        assert!(matches!(
            validate_support_attachment_set(&files),
            Err(SupportAttachmentError::Size)
        ));
    }
}

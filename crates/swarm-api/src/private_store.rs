//! Keeping a private on-disk store inside a bound without refusing new writes.
//!
//! Both attachment stores are caches of evidence the operator pasted or an
//! email carried. A cache that refuses once it is full is a cache that stops
//! working the moment it is used — which is exactly what happened: pasting a
//! screenshot returned "storage is full, try again", and trying again could
//! never succeed.
//!
//! Two rules, shared rather than copied. The allowance follows the disk, so it
//! is neither meaningless on a large drive nor a claim on space that is no
//! longer there as one fills. And when the incoming file does not fit, the
//! oldest give way rather than the newest being turned away.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Never smaller than this, whatever the disk says.
const MIN_STORE_BYTES: u64 = 128 * 1024 * 1024;
/// Never large enough to be the reason a disk filled.
const MAX_STORE_BYTES: u64 = 8 * 1024 * 1024 * 1024;

/// How much a store may hold right now, as a share of the free space beneath it.
///
/// Re-read on every write, so the answer tracks the disk rather than a number
/// chosen once. A filesystem that cannot be questioned gets the floor.
#[must_use]
pub(super) fn store_allowance(root: &Path, disk_percent: u64) -> u64 {
    let Ok(stats) = nix::sys::statvfs::statvfs(root) else {
        return MIN_STORE_BYTES;
    };
    stats
        .blocks_available()
        .saturating_mul(stats.fragment_size())
        .saturating_mul(disk_percent)
        .saturating_div(100)
        .clamp(MIN_STORE_BYTES, MAX_STORE_BYTES)
}

/// Files in the store, oldest first, with their sizes.
async fn entries_by_age(root: &Path) -> std::io::Result<Vec<(SystemTime, PathBuf, u64)>> {
    let mut entries = tokio::fs::read_dir(root).await?;
    let mut found = Vec::new();
    while let Some(entry) = entries.next_entry().await? {
        let metadata = entry.metadata().await?;
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

/// Evicts the oldest entries until the incoming file fits.
///
/// Returns whether it fits afterwards. Only a file larger than the whole
/// allowance can still fail, and every per-file ceiling here is a fraction of
/// the floor.
///
/// Eviction can break a link to an old file, which any expiry policy already
/// does; the difference is that this happens when the space is needed rather
/// than on a calendar.
pub(super) async fn make_room_for(
    root: &Path,
    incoming: u64,
    max_files: usize,
    disk_percent: u64,
) -> std::io::Result<bool> {
    let allowance = store_allowance(root, disk_percent);
    let (mut files, mut retained) = usage(root).await?;
    if files < max_files && retained.saturating_add(incoming) <= allowance {
        return Ok(true);
    }
    let mut oldest_first = entries_by_age(root).await?;
    while (files >= max_files || retained.saturating_add(incoming) > allowance)
        && !oldest_first.is_empty()
    {
        let (_, path, size) = oldest_first.remove(0);
        // A file already gone is the outcome wanted here, so its absence counts
        // the same as removing it.
        let _ = tokio::fs::remove_file(&path).await;
        files = files.saturating_sub(1);
        retained = retained.saturating_sub(size);
    }
    Ok(retained.saturating_add(incoming) <= allowance)
}

/// How many files the store holds, and how many bytes.
pub(super) async fn usage(root: &Path) -> std::io::Result<(usize, u64)> {
    let mut entries = tokio::fs::read_dir(root).await?;
    let (mut files, mut bytes) = (0usize, 0u64);
    while let Some(entry) = entries.next_entry().await? {
        let metadata = entry.metadata().await?;
        if metadata.is_file() {
            files = files.saturating_add(1);
            bytes = bytes.saturating_add(metadata.len());
        }
    }
    Ok((files, bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_oldest_give_way_so_the_newest_can_arrive() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        for index in 0..4u8 {
            tokio::fs::write(root.join(format!("{index}.bin")), [index; 16])
                .await
                .unwrap();
            // Distinct modification times, so oldest-first means something.
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }

        // A cap of two forces two evictions rather than a refusal.
        assert!(make_room_for(root, 16, 2, 5).await.unwrap());

        let (files, _) = usage(root).await.unwrap();
        assert!(files < 4, "room was made, not refused");
        // The newest survived; the oldest did not.
        assert!(root.join("3.bin").exists());
        assert!(!root.join("0.bin").exists());
    }

    #[tokio::test]
    async fn a_store_within_its_bound_evicts_nothing() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        tokio::fs::write(root.join("kept.bin"), [1; 16])
            .await
            .unwrap();

        assert!(make_room_for(root, 16, 64, 5).await.unwrap());

        assert!(root.join("kept.bin").exists());
    }

    #[test]
    fn the_allowance_follows_the_disk_between_a_floor_and_a_ceiling() {
        let directory = tempfile::tempdir().unwrap();
        let allowance = store_allowance(directory.path(), 5);
        assert!(allowance >= MIN_STORE_BYTES);
        assert!(allowance <= MAX_STORE_BYTES);
        // A filesystem that cannot be questioned still answers usefully.
        assert_eq!(
            store_allowance(Path::new("/definitely/not/a/mount"), 5),
            MIN_STORE_BYTES
        );
    }
}

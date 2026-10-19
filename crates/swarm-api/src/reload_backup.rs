use std::path::{Path, PathBuf};

use swarm_persistence::TaskStore;

/// Where a pre-migration copy is kept, under the database's own directory.
///
/// The operator already keeps hand-taken backups in
/// `~/.local/state/swarm/backups`, so this writes alongside them rather than
/// inventing a second place to look on the day someone needs one.
const BACKUP_DIRECTORY: &str = "backups";

/// How many pre-migration reload copies are kept, newest first.
///
/// The installer keeps three pre-update copies, and this matches it. Nothing
/// pruned these before: by 2026-10-18 the Keeper held 34, 2.5 GB, on a system
/// disk with 8 GB free, and every migrating reload added about 90 MB more.
const KEPT_RELOAD_BACKUPS: usize = 3;

/// Why a reload was refused, in words that name the step that failed.
///
/// "Could not reload" is what this must never say. A refusal the operator
/// cannot act on is a refusal they will work around.
#[derive(Debug)]
pub(crate) enum BackupError {
    Unreadable(String),
    NotWritten(String),
    Unverifiable(String),
}

impl std::fmt::Display for BackupError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (step, detail) = match self {
            Self::Unreadable(detail) => ("the database could not be read to back it up", detail),
            Self::NotWritten(detail) => ("the backup could not be written", detail),
            Self::Unverifiable(detail) => ("the backup was written but did not verify", detail),
        };
        write!(
            formatter,
            "this reload carries a schema migration, and {step}: {detail}"
        )
    }
}

/// What the CHECKOUT would migrate the live database to, if anything.
///
/// Reads the version the checkout declares rather than the one this running
/// binary was compiled with, and that distinction is the whole point: the
/// running binary already migrated the database to its own version at startup,
/// so comparing those two reports "no migration" for every reload — including
/// the ones that carry one.
///
/// `CURRENT_SCHEMA_VERSION` IS NOT A LITERAL. It is declared as an alias:
///
///   `const CURRENT_SCHEMA_VERSION: i64 = REPLY_ALLOWS_APPROVED_EXEMPTION_SCHEMA_VERSION;`
///
/// so resolving it takes two reads — the alias, then the constant it names.
/// Every migration here has added a fresh named constant and repointed the
/// alias, so a parse handling only a literal would read EVERY real release as
/// carrying no migration, and would skip the backup on exactly the reloads
/// that need one.
pub(crate) fn checkout_schema_version(checkout: &Path) -> Option<i64> {
    let source = std::fs::read_to_string(
        checkout
            .join("crates")
            .join("swarm-persistence")
            .join("src")
            .join("lib.rs"),
    )
    .ok()?;
    let declared = declared_value(&source, "CURRENT_SCHEMA_VERSION")?;
    match declared.parse::<i64>() {
        Ok(version) => Some(version),
        // An alias, so resolve the constant it points at.
        Err(_) => declared_value(&source, &declared)?.parse().ok(),
    }
}

/// The right-hand side of `const NAME: i64 = ...;`, with no evaluation.
fn declared_value(source: &str, name: &str) -> Option<String> {
    let needle = format!("const {name}: i64 = ");
    let value = source
        .lines()
        .find_map(|line| line.trim().strip_prefix(&needle)?.strip_suffix(';'))?;
    Some(value.trim().to_owned())
}

/// Whether the build about to replace this one would migrate the database.
///
/// A version this cannot read counts as YES. The two failure directions are
/// not symmetric: backing up a reload that did not need it costs a few
/// megabytes and some seconds, while skipping one that did costs the
/// operator's data with no remedy, because migrations here are forward-only.
/// So "cannot tell" resolves to backing up — including on the day someone
/// renames the constant and never thinks about this file.
fn would_migrate(checkout: &Path, live: i64) -> bool {
    checkout_schema_version(checkout).is_none_or(|declared| declared > live)
}

/// Takes a full copy of the database when — and only when — the build about to
/// replace this one would migrate it.
///
/// A CODE RELOAD IS SYMMETRIC AND A MIGRATION IS NOT. A wrong build is undone
/// by reloading again, a cost the operator has called "just development". A
/// migration rewrites their DATA, and migrations here are forward-only:
/// `RECENT_SCHEMA_STEPS` carries `undo_sql` for MODELLING an older database in
/// tests, not for reversing a live one. So the remedy is a restore, and a
/// restore exists only if a copy was taken beforehand — at exactly the moment
/// the person best placed to take it is busy doing something else.
///
/// These precautions were taken by hand on 2026-08-25 and cost seconds. They
/// depended entirely on the worker deciding to bother, which is the property
/// this removes.
///
/// # Errors
/// Returns an error when a migration is coming and the copy cannot be read,
/// written, or verified. The caller must REFUSE the reload on any of them: a
/// backup that warns and proceeds is the same as no backup on the only day it
/// matters.
pub(crate) fn back_up_before_migrating_reload(
    store: &TaskStore,
    database_directory: &Path,
    checkout: &Path,
    revision: &str,
    timestamp: &str,
) -> Result<Option<PathBuf>, BackupError> {
    let live = store
        .schema_version()
        .map_err(|error| BackupError::Unreadable(error.to_string()))?;
    let backups = database_directory.join(BACKUP_DIRECTORY);
    if !would_migrate(checkout, live) {
        prune_reload_backups(&backups, None);
        return Ok(None);
    }
    // Named for what it is FROM, because that is what someone restoring needs:
    // the version it holds, and the build that was about to change it.
    let destination = backups.join(format!("pre-v{live}-reload-{revision}-{timestamp}.sqlite3"));
    // SQLite's own backup API rather than a file copy. The database runs in WAL
    // mode, so copying the file alone takes it WITHOUT the committed pages
    // still in the -wal — a backup that opens cleanly and is quietly missing
    // the most recent work, the worst possible failure for a file nobody reads
    // until they need it.
    store
        .backup_to(&destination)
        .map_err(|error| BackupError::NotWritten(error.to_string()))?;
    swarm_persistence::verify_backup_at(&destination, live)
        .map_err(|error| BackupError::Unverifiable(error.to_string()))?;
    prune_reload_backups(&backups, Some(&destination));
    Ok(Some(destination))
}

/// Keeps the newest `KEPT_RELOAD_BACKUPS` pre-migration reload copies and
/// removes the rest, each with any `-shm` and `-wal` file beside it.
///
/// Only files named `pre-v…-reload-….sqlite3` are touched. The daily and
/// pre-update copies are the installer's to prune, and hand-taken copies are
/// the operator's. `kept` is the copy this reload just verified: it always
/// counts as one of the three, even if a clock moved and another file looks
/// newer. A copy that cannot be removed is left and logged, because cleanup
/// must never refuse a reload whose backup succeeded.
fn prune_reload_backups(backups: &Path, kept: Option<&Path>) {
    let Ok(entries) = std::fs::read_dir(backups) else {
        return;
    };
    let mut copies: Vec<(std::time::SystemTime, PathBuf)> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .filter(|entry| {
            entry.file_name().to_str().is_some_and(|name| {
                name.starts_with("pre-v") && name.contains("-reload-") && name.ends_with(".sqlite3")
            })
        })
        .filter(|entry| kept != Some(entry.path().as_path()))
        .filter_map(|entry| Some((entry.metadata().ok()?.modified().ok()?, entry.path())))
        .collect();
    copies.sort_by_key(|copy| std::cmp::Reverse(copy.0));
    let others_kept = KEPT_RELOAD_BACKUPS - usize::from(kept.is_some());
    for (_, path) in copies.into_iter().skip(others_kept) {
        for doomed in [
            path.clone(),
            PathBuf::from(format!("{}-shm", path.display())),
            PathBuf::from(format!("{}-wal", path.display())),
        ] {
            if let Err(error) = std::fs::remove_file(&doomed)
                && error.kind() != std::io::ErrorKind::NotFound
            {
                tracing::warn!(path = %doomed.display(), %error, "an old reload backup could not be removed");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A checkout containing only the one file this guard reads.
    fn checkout_declaring(source: &str) -> tempfile::TempDir {
        let checkout = tempfile::tempdir().unwrap();
        let directory = checkout
            .path()
            .join("crates")
            .join("swarm-persistence")
            .join("src");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("lib.rs"), source).unwrap();
        checkout
    }

    fn live_store() -> (tempfile::TempDir, TaskStore, i64) {
        let home = tempfile::tempdir().unwrap();
        let store = TaskStore::open(home.path().join("swarm.sqlite3")).unwrap();
        let version = store.schema_version().unwrap();
        (home, store, version)
    }

    /// The alias is the normal case here, not an edge case.
    ///
    /// Every migration in this codebase adds a named constant and repoints
    /// `CURRENT_SCHEMA_VERSION` at it, so a parser understanding only a literal
    /// would read every real release as carrying no migration — and would skip
    /// the backup on precisely the reloads that need one.
    #[test]
    fn the_declared_version_is_resolved_through_its_alias() {
        let checkout = checkout_declaring(
            "const SESSION_END_REASON_SCHEMA_VERSION: i64 = 92;\n\
             const REPLY_ALLOWS_APPROVED_EXEMPTION_SCHEMA_VERSION: i64 = 93;\n\
             const CURRENT_SCHEMA_VERSION: i64 = REPLY_ALLOWS_APPROVED_EXEMPTION_SCHEMA_VERSION;\n",
        );

        assert_eq!(checkout_schema_version(checkout.path()), Some(93));
    }

    /// The real file, so a future rename of the constant fails here rather than
    /// silently downgrading every reload to "no migration".
    #[test]
    fn this_repository_declares_a_version_this_guard_can_read() {
        // Use the actual source compiled with this test, but read it through
        // the production checkout guard in a native temporary directory. A
        // build-machine path is not necessarily valid on the execution host.
        let checkout = checkout_declaring(include_str!("../../swarm-persistence/src/lib.rs"));

        let declared = checkout_schema_version(checkout.path())
            .expect("CURRENT_SCHEMA_VERSION must stay readable by the reload backup guard");
        assert_eq!(
            declared,
            TaskStore::in_memory().unwrap().schema_version().unwrap()
        );
    }

    #[test]
    fn a_plain_literal_is_read_too() {
        let checkout = checkout_declaring("const CURRENT_SCHEMA_VERSION: i64 = 94;\n");

        assert_eq!(checkout_schema_version(checkout.path()), Some(94));
    }

    /// A reload that migrates takes a backup, and the backup is a real one.
    #[test]
    fn a_migrating_reload_backs_up_first() {
        let (home, store, live) = live_store();
        let checkout = checkout_declaring(&format!(
            "const CURRENT_SCHEMA_VERSION: i64 = {};\n",
            live + 1
        ));

        let backup = back_up_before_migrating_reload(
            &store,
            home.path(),
            checkout.path(),
            "89d04f485bd7",
            "20260825T193000Z",
        )
        .unwrap()
        .expect("a reload declaring a higher version must be backed up");

        assert!(backup.exists());
        // Named for the version it HOLDS, which is what someone restoring needs.
        assert!(
            backup
                .to_string_lossy()
                .contains(&format!("pre-v{live}-reload-89d04f485bd7")),
            "{}",
            backup.display()
        );
        // And it verifies as that version, rather than merely existing.
        swarm_persistence::verify_backup_at(&backup, live).unwrap();
    }

    /// Writes an old reload copy, with the given age in seconds, plus its
    /// `-shm` and `-wal` files.
    fn old_reload_copy(backups: &Path, version: i64, age_seconds: u64) -> PathBuf {
        let path = backups.join(format!(
            "pre-v{version}-reload-0ld0ld0ld0ld-2026090{version}.sqlite3"
        ));
        for file in [
            path.clone(),
            PathBuf::from(format!("{}-shm", path.display())),
            PathBuf::from(format!("{}-wal", path.display())),
        ] {
            std::fs::write(&file, b"old").unwrap();
            std::fs::File::options()
                .write(true)
                .open(&file)
                .unwrap()
                .set_modified(
                    std::time::SystemTime::now() - std::time::Duration::from_secs(age_seconds),
                )
                .unwrap();
        }
        path
    }

    /// The Keeper held 34 of these, 2.5 GB, because nothing removed them. A
    /// migrating reload keeps its own copy and the two newest older ones, and
    /// leaves every other kind of backup alone.
    #[test]
    fn a_migrating_reload_keeps_three_reload_copies_and_nothing_else_moves() {
        let (home, store, live) = live_store();
        let backups = home.path().join(BACKUP_DIRECTORY);
        std::fs::create_dir_all(&backups).unwrap();
        let oldest = old_reload_copy(&backups, 1, 4_000);
        let older = old_reload_copy(&backups, 2, 3_000);
        let newer = old_reload_copy(&backups, 3, 2_000);
        let newest = old_reload_copy(&backups, 4, 1_000);
        let daily = backups.join("daily-20261017.sqlite3");
        let update = backups.join("pre-update-1.17.4.sqlite3");
        let by_hand = backups.join("before-a-risky-change.sqlite3");
        for other in [&daily, &update, &by_hand] {
            std::fs::write(other, b"keep").unwrap();
        }
        let checkout = checkout_declaring(&format!(
            "const CURRENT_SCHEMA_VERSION: i64 = {};\n",
            live + 1
        ));

        let written = back_up_before_migrating_reload(
            &store,
            home.path(),
            checkout.path(),
            "89d04f485bd7",
            "20261018T200000Z",
        )
        .unwrap()
        .expect("a migrating reload takes a copy");

        assert!(written.exists());
        assert!(newest.exists() && newer.exists());
        for gone in [&oldest, &older] {
            assert!(!gone.exists(), "{} should be pruned", gone.display());
            assert!(!PathBuf::from(format!("{}-shm", gone.display())).exists());
            assert!(!PathBuf::from(format!("{}-wal", gone.display())).exists());
        }
        for other in [&daily, &update, &by_hand] {
            assert!(other.exists(), "{} is not a reload copy", other.display());
        }
    }

    /// A reload with no migration takes no copy, but still trims the pile to
    /// the newest three, so the copies already there do not wait for the next
    /// schema change to go.
    #[test]
    fn a_reload_without_a_migration_trims_to_the_newest_three() {
        let (home, store, live) = live_store();
        let backups = home.path().join(BACKUP_DIRECTORY);
        std::fs::create_dir_all(&backups).unwrap();
        let copies: Vec<_> = (1..=5)
            .map(|version| {
                old_reload_copy(
                    &backups,
                    version,
                    10_000 - u64::try_from(version).unwrap() * 1_000,
                )
            })
            .collect();
        let checkout =
            checkout_declaring(&format!("const CURRENT_SCHEMA_VERSION: i64 = {live};\n"));

        assert!(
            back_up_before_migrating_reload(&store, home.path(), checkout.path(), "r", "t")
                .unwrap()
                .is_none()
        );

        let survivors: Vec<bool> = copies.iter().map(|copy| copy.exists()).collect();
        assert_eq!(survivors, [false, false, true, true, true]);
    }

    /// The other half of the ablation: no migration, no backup, nothing touched.
    #[test]
    fn a_reload_without_a_migration_takes_no_backup() {
        let (home, store, live) = live_store();
        let checkout =
            checkout_declaring(&format!("const CURRENT_SCHEMA_VERSION: i64 = {live};\n"));

        let taken = back_up_before_migrating_reload(
            &store,
            home.path(),
            checkout.path(),
            "89d04f485bd7",
            "20260825T193000Z",
        )
        .unwrap();

        assert!(taken.is_none());
        // Not merely "no path returned" — the directory is never created, so an
        // ordinary reload leaves no trace at all.
        assert!(!home.path().join(BACKUP_DIRECTORY).exists());
    }

    /// A checkout this guard cannot read is treated as carrying a migration.
    #[test]
    fn a_version_that_cannot_be_read_is_assumed_to_migrate() {
        let (home, store, _) = live_store();
        let unreadable = checkout_declaring("const SOMETHING_ELSE_ENTIRELY: i64 = 93;\n");

        assert_eq!(checkout_schema_version(unreadable.path()), None);
        assert!(
            back_up_before_migrating_reload(
                &store,
                home.path(),
                unreadable.path(),
                "89d04f485bd7",
                "20260825T193000Z",
            )
            .unwrap()
            .is_some(),
            "an unreadable declared version must back up rather than skip"
        );
    }

    /// A backup that cannot be written REFUSES, and says which step failed.
    #[test]
    fn a_backup_that_cannot_be_written_refuses_and_names_the_step() {
        let (home, store, live) = live_store();
        let checkout = checkout_declaring(&format!(
            "const CURRENT_SCHEMA_VERSION: i64 = {};\n",
            live + 1
        ));
        // A plain file where the backups directory needs to be, so the write
        // fails the way a permissions problem or a full disk would.
        std::fs::write(home.path().join(BACKUP_DIRECTORY), "not a directory").unwrap();

        let refusal = back_up_before_migrating_reload(
            &store,
            home.path(),
            checkout.path(),
            "89d04f485bd7",
            "20260825T193000Z",
        )
        .expect_err("a failed backup must refuse, not warn and proceed");

        let message = refusal.to_string();
        // The operator has to be able to act on this. "Could not reload" is the
        // message this exists to avoid.
        assert!(message.contains("schema migration"), "{message}");
        assert!(message.contains("could not be written"), "{message}");
    }

    /// Writing bytes is not the same as having a backup.
    #[test]
    fn a_backup_that_is_not_the_database_it_claims_does_not_verify() {
        let (home, store, live) = live_store();
        let impostor = home.path().join("impostor.sqlite3");
        store.backup_to(&impostor).unwrap();

        let refusal = swarm_persistence::verify_backup_at(&impostor, live + 41)
            .expect_err("a version mismatch must not verify");

        assert!(refusal.to_string().contains("schema version"), "{refusal}");
    }
}

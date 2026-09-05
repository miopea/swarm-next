//! Engine-owned startup hook overlay; never edits provider/user settings in place.
use serde_json::{Value, json};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

const MAX_SETTINGS_BYTES: u64 = 1024 * 1024;

fn add_hook(document: &mut Value, executable: &Path) -> Result<(), String> {
    let executable = executable.to_str().ok_or("helper path is not UTF-8")?;
    // The provider invokes command hooks through a shell. Single-quote the
    // complete path, including embedded apostrophes, without interpolating env.
    let command = format!(
        "'{}' provider-session-start",
        executable.replace('\'', "'\\''")
    );
    let hooks = document
        .as_object_mut()
        .ok_or("settings are not an object")?
        .entry("hooks")
        .or_insert_with(|| json!({}));
    let starts = hooks
        .as_object_mut()
        .ok_or("hooks are not an object")?
        .entry("SessionStart")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or("SessionStart is not an array")?;
    let entry =
        json!({"matcher": "startup|resume", "hooks": [{"type": "command", "command": command}]});
    if !starts.contains(&entry) {
        starts.push(entry);
    }
    let ends = hooks
        .as_object_mut()
        .ok_or("hooks are not an object")?
        .entry("SessionEnd")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or("SessionEnd is not an array")?;
    let command = format!(
        "'{}' provider-resume-end",
        executable.replace('\'', "'\\''")
    );
    let entry = json!({"matcher":"resume", "hooks":[{"type":"command","command":command}]});
    if !ends.contains(&entry) {
        ends.push(entry);
    }
    Ok(())
}

pub(super) fn read_settings(path: &Path) -> Result<Value, String> {
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|_| "settings unavailable")?
        .take(MAX_SETTINGS_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "settings unreadable")?;
    if bytes.len() as u64 > MAX_SETTINGS_BYTES {
        return Err("settings exceed limit".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| "settings invalid".into())
}

pub(super) fn startup_settings(
    base: Option<&Path>,
    mcp_config: &Path,
    executable: &Path,
) -> Result<PathBuf, String> {
    // A running process can outlive its unlinked binary. Never publish the
    // resulting `current_exe()` path (including a Linux " (deleted)" suffix)
    // as a callback command or silently substitute another release's helper.
    let helper = fs::metadata(executable).map_err(|_| "helper executable unavailable")?;
    if !executable.is_absolute() || !helper.is_file() || helper.permissions().mode() & 0o111 == 0 {
        return Err("helper executable unavailable".into());
    }
    let mut document = if let Some(base) = base {
        read_settings(base)?
    } else {
        json!({})
    };
    add_hook(&mut document, executable)?;
    let bytes = serde_json::to_vec_pretty(&document).map_err(|_| "settings invalid")?;
    if bytes.len() as u64 > MAX_SETTINGS_BYTES {
        return Err("settings exceed limit".into());
    }
    let target = mcp_config.with_extension("startup.settings.json");
    let temporary = target.with_extension(format!("{}.tmp", swarm_domain::WorkerSessionId::new()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, &target)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(|_| "startup settings unavailable")?;
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlay_keeps_operator_hooks_and_permissions_and_is_idempotent() {
        let mut document = json!({"permissions":{"allow":["Edit"],"deny":["Bash(rm:*)"]},
            "hooks":{"SessionStart":[{"matcher":"startup","hooks":[{"type":"command","command":"echo existing"}]}],"Stop":[]},
            "disableAllHooks":true});
        let original = document.clone();
        let executable = Path::new("/opt/bee's hive/host");
        add_hook(&mut document, executable).unwrap();
        add_hook(&mut document, executable).unwrap();
        assert_eq!(document["permissions"], original["permissions"]);
        assert_eq!(document["disableAllHooks"], true);
        assert_eq!(document["hooks"]["Stop"], original["hooks"]["Stop"]);
        let starts = document["hooks"]["SessionStart"].as_array().unwrap();
        assert_eq!(starts.len(), 2);
        assert_eq!(document["hooks"]["SessionEnd"].as_array().unwrap().len(), 1);
        assert_eq!(starts[0], original["hooks"]["SessionStart"][0]);
        assert_eq!(
            starts[1]["hooks"][0]["command"],
            "'/opt/bee'\\''s hive/host' provider-session-start"
        );
    }

    #[test]
    fn no_grants_still_gets_private_hook_overlay_and_bad_input_preserves_it() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let mcp = root.path().join("worker.json");
        let executable = std::env::current_exe().unwrap();
        let target = startup_settings(None, &mcp, &executable).unwrap();
        let before = fs::read(&target).unwrap();
        assert_eq!(
            fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let invalid = root.path().join("bad.json");
        fs::write(&invalid, "{broken").unwrap();
        assert!(startup_settings(Some(&invalid), &mcp, &executable).is_err());
        assert_eq!(fs::read(&target).unwrap(), before);
        assert_eq!(fs::read_to_string(invalid).unwrap(), "{broken");
    }

    #[test]
    fn malformed_hooks_and_oversized_settings_are_not_overwritten() {
        for mut document in [
            json!([]),
            json!({"hooks":[]}),
            json!({"hooks":{"SessionStart":{}}}),
        ] {
            assert!(add_hook(&mut document, Path::new("/host")).is_err());
        }
        let root = tempfile::tempdir().unwrap();
        let base = root.path().join("large.json");
        fs::write(
            &base,
            vec![b' '; usize::try_from(MAX_SETTINGS_BYTES).unwrap() + 1],
        )
        .unwrap();
        let mcp = root.path().join("worker.json");
        assert!(startup_settings(Some(&base), &mcp, &std::env::current_exe().unwrap()).is_err());
        assert!(!mcp.with_extension("startup.settings.json").exists());
    }

    #[test]
    fn unavailable_helper_cannot_replace_an_existing_overlay() {
        let root = tempfile::tempdir().unwrap();
        let mcp = root.path().join("worker.json");
        let executable = std::env::current_exe().unwrap();
        let target = startup_settings(None, &mcp, &executable).unwrap();
        let original = fs::read(&target).unwrap();
        let non_executable = root.path().join("host");
        fs::write(&non_executable, "not executable").unwrap();
        fs::set_permissions(&non_executable, fs::Permissions::from_mode(0o600)).unwrap();
        for invalid in [
            root.path().join("missing"),
            root.path().join("host (deleted)"),
            root.path().to_path_buf(),
            non_executable,
            PathBuf::from("relative-host"),
        ] {
            assert_eq!(
                startup_settings(None, &mcp, &invalid).unwrap_err(),
                "helper executable unavailable"
            );
            assert_eq!(fs::read(&target).unwrap(), original);
        }
        // A later valid launch can regenerate its own overlay.
        assert_eq!(startup_settings(None, &mcp, &executable).unwrap(), target);
    }
}

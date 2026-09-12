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
    // ⚠️ WITHOUT THESE TWO, ANSWERING IN A TERMINAL REACHES SWARM THROUGH NO
    // PATH AT ALL, and that is not a theory — it was measured on the operator's
    // own Hive on 2026-09-12. They answered a question in a worker's terminal
    // three times; native_operator_interviews stayed at 0 for six minutes and
    // the decision stayed pending. Their words: "It is either not working or is
    // so slow it is useless for the ux."
    //
    // The whole bridge downstream of this — capture, retain, bind, record,
    // resolve — was built, tested and released believing step one was wired.
    // The capture code SHIPS in the engine binary; nothing ever fed it. The
    // host does not watch the PTY: it has to be TOLD, by this hook, and
    // `add_hook` only ever installed SessionStart and SessionEnd.
    //
    // BOTH EVENTS ARE REQUIRED and they are not redundant. read_claude_interview
    // treats PreToolUse as the provisional Requested phase and PostToolUse as
    // Completed, and only the second carries tool_response — the answers. The
    // capture holds the provisional one and releases it solely when the final
    // batch matches exactly, which is what makes final_result ExactBatch mean
    // anything. Install one without the other and it silently captures nothing.
    let command = format!("'{}' provider-interview", executable.replace('\'', "'\\''"));
    // ⚠️ THREE EVENTS, AND THE THIRD IS THE ONE THAT MAKES ANY OF IT COUNT.
    //
    // PreToolUse and PostToolUse only ever produce a PROVISIONAL observation.
    // Nothing is ever captured until a PostToolBatch arrives: finalize() is the
    // only thing that stamps NativeInterviewFinalResult::ExactBatch, and the
    // application layer treats absence of that stamp as UNCHECKED and refuses
    // to resolve anything with it. matches_final_batch rejects any payload whose
    // hook_event_name is not exactly "PostToolBatch", and observe_native_interview
    // will not even enter its finalizing branch without one.
    //
    // I shipped bbfda46c with only the first two. The result was not a partial
    // feature: it was zero captures, for every worker, with the host logging a
    // broken pipe per question and Claude showing the operator
    // "PostToolUse:AskUserQuestion hook error ... provider startup evidence
    // unavailable". They reported that screen on 2026-09-12 and it is what
    // pointed here.
    //
    // PostToolBatch carries a batch of tool calls rather than one tool, so it
    // takes no tool matcher — matching on "AskUserQuestion" would silently
    // never fire, which is the same failure again in a new costume.
    for event in ["PreToolUse", "PostToolUse"] {
        let entry = json!({
            "matcher": "AskUserQuestion",
            "hooks": [{"type": "command", "command": command}],
        });
        let slot = hooks
            .as_object_mut()
            .ok_or("hooks are not an object")?
            .entry(event)
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .ok_or("hook event is not an array")?;
        if !slot.contains(&entry) {
            slot.push(entry);
        }
    }
    let batch = json!({"hooks": [{"type": "command", "command": command}]});
    let slot = hooks
        .as_object_mut()
        .ok_or("hooks are not an object")?
        .entry("PostToolBatch")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or("hook event is not an array")?;
    if !slot.contains(&batch) {
        slot.push(batch);
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

    /// The interview hooks are installed, and BOTH of them.
    ///
    /// ⚠️ THE WHOLE NATIVE-ANSWER BRIDGE WAS DEAD WITHOUT THESE AND EVERY TEST
    /// PASSED. Capture, retain, bind, record and resolve were built, released
    /// and believed wired; the engine binary contains the capture code. Nothing
    /// fed it. The host does not watch the PTY — it has to be TOLD by this
    /// hook, and `add_hook` installed only `SessionStart` and `SessionEnd`.
    ///
    /// Measured on the operator's Hive 2026-09-12: three answers typed in a
    /// worker's terminal, `native_operator_interviews` stayed at 0 for six
    /// minutes, decision stayed pending. "It is either not working or is so slow
    /// it is useless for the ux."
    ///
    /// BOTH EVENTS, because `read_claude_interview` reads `PreToolUse` as the
    /// provisional phase and `PostToolUse` as completed, and only the second
    /// carries the answers. One without the other captures nothing, silently —
    /// so asserting on one would leave exactly the hole this came from.
    #[test]
    fn the_overlay_installs_both_interview_hooks_or_the_bridge_is_fed_nothing() {
        let mut document = json!({});
        let executable = Path::new("/opt/hive/host");
        add_hook(&mut document, executable).unwrap();

        for event in ["PreToolUse", "PostToolUse"] {
            let entries = document["hooks"][event]
                .as_array()
                .unwrap_or_else(|| panic!("{event} is missing entirely"));
            let interview = entries
                .iter()
                .find(|entry| entry["matcher"] == "AskUserQuestion")
                .unwrap_or_else(|| panic!("{event} has no AskUserQuestion hook"));
            assert_eq!(
                interview["hooks"][0]["command"], "'/opt/hive/host' provider-interview",
                "{event} must call the interview subcommand"
            );
        }

        // ⚠️ THE ONE THAT MAKES ANY OF IT COUNT. Without PostToolBatch nothing
        // is ever finalised, so nothing is ever captured — which is exactly
        // what bbfda46c shipped, and the test above passed the whole time
        // because it only asked about the two provisional events.
        let batch = document["hooks"]["PostToolBatch"]
            .as_array()
            .expect("PostToolBatch is missing: nothing will ever be finalised");
        assert_eq!(batch.len(), 1);
        assert_eq!(
            batch[0]["hooks"][0]["command"],
            "'/opt/hive/host' provider-interview"
        );
        // No tool matcher: a batch is not one tool, and matching it on
        // "AskUserQuestion" would silently never fire.
        assert!(
            batch[0].get("matcher").is_none(),
            "PostToolBatch must not carry a tool matcher"
        );
    }

    /// Installed twice is installed once. A worker restart re-runs this.
    #[test]
    fn installing_the_interview_hooks_twice_does_not_duplicate_them() {
        let mut document = json!({});
        let executable = Path::new("/opt/hive/host");
        add_hook(&mut document, executable).unwrap();
        add_hook(&mut document, executable).unwrap();
        for event in ["PreToolUse", "PostToolUse"] {
            let count = document["hooks"][event]
                .as_array()
                .unwrap()
                .iter()
                .filter(|entry| entry["matcher"] == "AskUserQuestion")
                .count();
            assert_eq!(count, 1, "{event} gained a duplicate");
        }
        assert_eq!(
            document["hooks"]["PostToolBatch"].as_array().unwrap().len(),
            1
        );
    }

    /// An operator's own `PreToolUse` hooks survive, because this file merges.
    #[test]
    fn the_operator_s_own_pre_tool_use_hooks_are_kept() {
        let mut document = json!({"hooks":{"PreToolUse":[
            {"matcher":"Bash","hooks":[{"type":"command","command":"echo mine"}]}]}});
        add_hook(&mut document, Path::new("/opt/hive/host")).unwrap();
        let entries = document["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0]["matcher"], "Bash");
        assert_eq!(entries[0]["hooks"][0]["command"], "echo mine");
    }

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

//! Writing a program that a test then runs.

use std::{
    io::Write,
    path::Path,
    process::{Command, Stdio},
};

/// Writes `contents` to `path` as a program only its owner can run.
///
/// ⚠️ NOT `fs::write` FOLLOWED BY `set_permissions`. Linux refuses to run a
/// file that anything holds open for writing ("Text file busy", ETXTBSY), and
/// a file this process holds open is copied into every child another test
/// thread starts, until that child runs its own program. Tests here run in
/// parallel and many of them start processes, so a program written in this
/// process could be refused at random. A shell this function waits for writes
/// it instead, so this process never holds the file open at all.
pub(crate) fn write_executable(path: &Path, contents: &str) {
    let mut shell = Command::new("sh")
        .args(["-c", "umask 077 && cat > \"$1\" && chmod 700 \"$1\"", "sh"])
        .arg(path)
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    shell
        .stdin
        .take()
        .unwrap()
        .write_all(contents.as_bytes())
        .unwrap();
    assert!(
        shell.wait().unwrap().success(),
        "could not write {}",
        path.display()
    );
}

#[test]
fn a_written_program_runs_while_other_threads_start_processes() {
    use std::os::unix::fs::PermissionsExt;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    const ROUNDS: usize = 200;
    let stop = Arc::new(AtomicBool::new(false));
    let starters: Vec<_> = (0..4)
        .map(|_| {
            let stop = Arc::clone(&stop);
            std::thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    Command::new("true").status().unwrap();
                }
            })
        })
        .collect();
    let directory = tempfile::tempdir().unwrap();
    let refused: Vec<String> = (0..ROUNDS)
        .filter_map(|round| {
            let program = directory.path().join(round.to_string());
            write_executable(&program, "#!/bin/sh\nexit 0\n");
            match Command::new(&program).status() {
                Ok(status) if status.success() => None,
                Ok(status) => Some(format!("round {round}: {status}")),
                Err(error) => Some(format!("round {round}: {error}")),
            }
        })
        .collect();
    stop.store(true, Ordering::Relaxed);
    for starter in starters {
        starter.join().unwrap();
    }
    assert!(
        refused.is_empty(),
        "{} of {ROUNDS} programs could not be run: {refused:?}",
        refused.len()
    );
    let written = directory.path().join("0");
    assert_eq!(
        std::fs::metadata(&written).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        std::fs::read_to_string(&written).unwrap(),
        "#!/bin/sh\nexit 0\n"
    );
}

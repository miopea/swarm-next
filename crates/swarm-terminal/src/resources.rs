use serde::{Deserialize, Serialize};

#[cfg(target_os = "linux")]
use std::collections::{HashMap, HashSet, VecDeque};

/// Content-free resource evidence owned by one runtime process.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProcessResourceSample {
    pub resident_memory_bytes: Option<u64>,
    /// Resident memory for this process and every descendant it owns.
    /// Provider CLIs frequently use several processes, so this is the useful
    /// operator-facing number while `resident_memory_bytes` remains the broker
    /// process's own cost.
    #[serde(default)]
    pub process_tree_resident_memory_bytes: Option<u64>,
    #[serde(default)]
    pub process_tree_process_count: Option<u32>,
}

/// Samples the current process without starting a polling task or retaining history.
#[must_use]
pub fn sample_current_process() -> ProcessResourceSample {
    sample_process_tree(std::process::id())
}

#[cfg(target_os = "linux")]
#[must_use]
pub fn sample_process_tree(root_pid: u32) -> ProcessResourceSample {
    let processes = linux_process_snapshot();
    let resident_memory_bytes = processes.get(&root_pid).map(|(_, rss)| *rss);
    let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
    for (pid, (parent, _)) in &processes {
        children.entry(*parent).or_default().push(*pid);
    }
    let mut pending = VecDeque::from([root_pid]);
    let mut visited = HashSet::new();
    let mut total = 0_u64;
    while let Some(pid) = pending.pop_front() {
        if !visited.insert(pid) {
            continue;
        }
        if let Some((_, rss)) = processes.get(&pid) {
            total = total.saturating_add(*rss);
        }
        if let Some(descendants) = children.get(&pid) {
            pending.extend(descendants);
        }
    }
    ProcessResourceSample {
        resident_memory_bytes,
        process_tree_resident_memory_bytes: (!visited.is_empty()).then_some(total),
        process_tree_process_count: u32::try_from(visited.len()).ok(),
    }
}

#[cfg(not(target_os = "linux"))]
#[must_use]
pub const fn sample_process_tree(_root_pid: u32) -> ProcessResourceSample {
    ProcessResourceSample {
        resident_memory_bytes: None,
        process_tree_resident_memory_bytes: None,
        process_tree_process_count: None,
    }
}

#[cfg(target_os = "linux")]
fn linux_process_snapshot() -> HashMap<u32, (u32, u64)> {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return HashMap::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let pid = entry.file_name().to_string_lossy().parse::<u32>().ok()?;
            let status = std::fs::read_to_string(entry.path().join("status")).ok()?;
            let (parent, rss) = parse_linux_status(&status)?;
            Some((pid, (parent, rss)))
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn parse_linux_status(status: &str) -> Option<(u32, u64)> {
    let parent = status
        .lines()
        .find_map(|line| line.strip_prefix("PPid:")?.trim().parse().ok())?;
    let value = status.lines().find_map(|line| {
        line.strip_prefix("VmRSS:")?
            .trim()
            .split_once(char::is_whitespace)
    })?;
    let rss = (value.1.trim() == "kB")
        .then(|| value.0.parse::<u64>().ok())
        .flatten()?
        .checked_mul(1024)?;
    Some((parent, rss))
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn parses_only_the_linux_resident_set_field() {
        let status = "Name:\tswarm-api\nPPid:\t42\nVmSize:\t999 kB\nVmRSS:\t12345 kB\n";
        assert_eq!(parse_linux_status(status), Some((42, 12_641_280)));
        assert_eq!(parse_linux_status("PPid:\t42\nVmRSS:\t12 MB\n"), None);
        assert_eq!(parse_linux_status("PPid:\t42\nVmSize:\t12 kB\n"), None);
    }

    #[test]
    fn current_process_sample_is_available_on_linux() {
        let sample = sample_current_process();
        assert!(sample.resident_memory_bytes.is_some());
        assert!(sample.process_tree_resident_memory_bytes >= sample.resident_memory_bytes);
        assert!(
            sample
                .process_tree_process_count
                .is_some_and(|count| count >= 1)
        );
    }
}

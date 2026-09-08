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
    ProcessResourceSnapshot::capture().sample(root_pid)
}

/// One content-free, call-owned OS snapshot shared by a bounded fleet listing.
/// No cross-request cache, timer or process command line is retained.
pub(crate) struct ProcessResourceSnapshot {
    #[cfg(target_os = "linux")]
    processes: HashMap<u32, (u32, u64)>,
    #[cfg(target_os = "linux")]
    children: HashMap<u32, Vec<u32>>,
}

impl ProcessResourceSnapshot {
    #[cfg(target_os = "linux")]
    pub(crate) fn capture() -> Self {
        Self::from_processes(linux_process_snapshot())
    }

    #[cfg(target_os = "linux")]
    fn from_processes(processes: HashMap<u32, (u32, u64)>) -> Self {
        let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
        for (pid, (parent, _)) in &processes {
            children.entry(*parent).or_default().push(*pid);
        }
        Self {
            processes,
            children,
        }
    }

    #[cfg(not(target_os = "linux"))]
    pub(crate) const fn capture() -> Self {
        Self {}
    }

    #[cfg(not(target_os = "linux"))]
    pub(crate) fn sample(&self, _root_pid: u32) -> ProcessResourceSample {
        ProcessResourceSample::default()
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn sample(&self, root_pid: u32) -> ProcessResourceSample {
        let Some((_, resident)) = self.processes.get(&root_pid) else {
            // Missing/vanished roots are unavailable, not a fabricated zero-byte tree.
            return ProcessResourceSample::default();
        };
        let mut pending = VecDeque::from([root_pid]);
        let mut visited = HashSet::new();
        let mut total = 0_u64;
        while let Some(pid) = pending.pop_front() {
            if !visited.insert(pid) {
                continue;
            }
            if let Some((_, rss)) = self.processes.get(&pid) {
                total = total.saturating_add(*rss);
            }
            if let Some(descendants) = self.children.get(&pid) {
                pending.extend(descendants);
            }
        }
        ProcessResourceSample {
            resident_memory_bytes: Some(*resident),
            process_tree_resident_memory_bytes: (!visited.is_empty()).then_some(total),
            process_tree_process_count: u32::try_from(visited.len()).ok(),
        }
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
    let mut processes = HashMap::new();
    for (seen, entry) in entries.flatten().enumerate() {
        // Refuse an oversized scan rather than publish truncated tree totals.
        if seen >= 65_536 {
            return HashMap::new();
        }
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        let Ok(status) = std::fs::read_to_string(entry.path().join("status")) else {
            continue;
        };
        if let Some(sample) = parse_linux_status(&status) {
            processes.insert(pid, sample);
        }
    }
    processes
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
    #[ignore = "explicit bounded Linux process-scan profiling, not a timing assertion"]
    fn compare_seventeen_separate_scans_with_one_fleet_snapshot() {
        let started = std::time::Instant::now();
        for _ in 0..17 {
            assert!(
                std::hint::black_box(sample_current_process())
                    .resident_memory_bytes
                    .is_some()
            );
        }
        let separate = started.elapsed();
        let started = std::time::Instant::now();
        let snapshot = ProcessResourceSnapshot::capture();
        for _ in 0..17 {
            assert!(
                std::hint::black_box(snapshot.sample(std::process::id()))
                    .resident_memory_bytes
                    .is_some()
            );
        }
        eprintln!(
            "separate_scans=17 separate_ms={:.3} shared_scans=1 shared_ms={:.3}",
            separate.as_secs_f64() * 1000.0,
            started.elapsed().as_secs_f64() * 1000.0
        );
    }

    #[test]
    fn shared_snapshot_preserves_independent_trees_and_missing_roots_are_unknown() {
        let snapshot = ProcessResourceSnapshot::from_processes(HashMap::from([
            (10, (1, 100)),
            (11, (10, 200)),
            (12, (11, 300)),
            (20, (1, 400)),
            (21, (20, 500)),
        ]));
        assert_eq!(
            snapshot.sample(10),
            ProcessResourceSample {
                resident_memory_bytes: Some(100),
                process_tree_resident_memory_bytes: Some(600),
                process_tree_process_count: Some(3),
            }
        );
        assert_eq!(
            snapshot.sample(20).process_tree_resident_memory_bytes,
            Some(900)
        );
        assert_eq!(snapshot.sample(999), ProcessResourceSample::default());
        // Repeated callers reuse exactly the same reading, with no mutable cache.
        assert_eq!(snapshot.sample(10), snapshot.sample(10));
    }

    #[test]
    fn malformed_parent_cycles_are_bounded_and_count_each_process_once() {
        let snapshot = ProcessResourceSnapshot::from_processes(HashMap::from([
            (10, (11, 100)),
            (11, (10, 200)),
        ]));
        assert_eq!(snapshot.sample(10).process_tree_process_count, Some(2));
        assert_eq!(
            snapshot.sample(10).process_tree_resident_memory_bytes,
            Some(300)
        );
    }

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

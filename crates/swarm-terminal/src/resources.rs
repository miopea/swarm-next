use serde::{Deserialize, Serialize};

/// Content-free resource evidence owned by one runtime process.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProcessResourceSample {
    pub resident_memory_bytes: Option<u64>,
}

/// Samples the current process without starting a polling task or retaining history.
#[must_use]
pub fn sample_current_process() -> ProcessResourceSample {
    ProcessResourceSample {
        resident_memory_bytes: resident_memory_bytes(),
    }
}

#[cfg(target_os = "linux")]
fn resident_memory_bytes() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    parse_linux_status_rss(&status)
}

#[cfg(not(target_os = "linux"))]
const fn resident_memory_bytes() -> Option<u64> {
    None
}

#[cfg(target_os = "linux")]
fn parse_linux_status_rss(status: &str) -> Option<u64> {
    let value_kib = status.lines().find_map(|line| {
        let value = line.strip_prefix("VmRSS:")?.trim();
        let (number, unit) = value.split_once(char::is_whitespace)?;
        (unit.trim() == "kB")
            .then(|| number.parse::<u64>().ok())
            .flatten()
    })?;
    value_kib.checked_mul(1024)
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn parses_only_the_linux_resident_set_field() {
        let status = "Name:\tswarm-api\nVmSize:\t999 kB\nVmRSS:\t12345 kB\n";
        assert_eq!(parse_linux_status_rss(status), Some(12_641_280));
        assert_eq!(parse_linux_status_rss("VmRSS:\t12 MB\n"), None);
        assert_eq!(parse_linux_status_rss("VmSize:\t12 kB\n"), None);
    }

    #[test]
    fn current_process_sample_is_available_on_linux() {
        assert!(sample_current_process().resident_memory_bytes.is_some());
    }
}

use serde::Serialize;

/// Evidence severity, not authority to stop a worker or an attribution of cause.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourcePressure {
    #[default]
    Normal,
    Advisory,
    Critical,
    Unavailable,
}

fn valid_percent(value: Option<f64>) -> Option<f64> {
    value.filter(|value| value.is_finite() && (0.0..=100.0).contains(value))
}

/// The memory-stall observation is independent of CPU and used memory.
#[must_use]
pub fn memory_stall_pressure(stall_percent: Option<f64>) -> ResourcePressure {
    match valid_percent(stall_percent) {
        Some(value) if value >= 10.0 => ResourcePressure::Critical,
        Some(value) if value >= 2.0 => ResourcePressure::Advisory,
        Some(_) => ResourcePressure::Normal,
        None => ResourcePressure::Unavailable,
    }
}

/// Preserve the established 85/95-percent memory and 2/10-percent PSI policy.
/// Missing use and a non-elevated stall observation cannot establish capacity.
#[must_use]
pub fn machine_memory_pressure(
    used_percent: Option<f64>,
    stall_percent: Option<f64>,
) -> ResourcePressure {
    let used = valid_percent(used_percent);
    let stall = memory_stall_pressure(stall_percent);
    if stall == ResourcePressure::Critical || used.is_some_and(|used| used >= 95.0) {
        ResourcePressure::Critical
    } else if stall == ResourcePressure::Advisory || used.is_some_and(|used| used >= 85.0) {
        ResourcePressure::Advisory
    } else if used.is_some() {
        ResourcePressure::Normal
    } else {
        ResourcePressure::Unavailable
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn independent_memory_evidence_preserves_thresholds_and_recovery() {
        for (used, stall, expected) in [
            (43.0, 0.0, ResourcePressure::Normal),
            (85.0, 0.0, ResourcePressure::Advisory),
            (95.0, 0.0, ResourcePressure::Critical),
            (43.0, 2.0, ResourcePressure::Advisory),
            (43.0, 10.0, ResourcePressure::Critical),
            (43.0, 0.0, ResourcePressure::Normal),
        ] {
            assert_eq!(machine_memory_pressure(Some(used), Some(stall)), expected);
        }
        assert_eq!(memory_stall_pressure(Some(0.0)), ResourcePressure::Normal);
    }

    #[test]
    fn invalid_or_absent_memory_evidence_is_not_normal() {
        for missing in [
            None,
            Some(f64::NAN),
            Some(f64::INFINITY),
            Some(-1.0),
            Some(101.0),
        ] {
            assert_eq!(
                memory_stall_pressure(missing),
                ResourcePressure::Unavailable
            );
            assert_eq!(
                machine_memory_pressure(missing, Some(0.0)),
                ResourcePressure::Unavailable
            );
            assert_eq!(
                machine_memory_pressure(missing, Some(10.0)),
                ResourcePressure::Critical
            );
            assert_eq!(
                machine_memory_pressure(Some(95.0), missing),
                ResourcePressure::Critical
            );
        }
    }
}

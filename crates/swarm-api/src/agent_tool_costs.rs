//! What each agent tool costs to answer, counted in this process (ADR 0144).
//!
//! A tally per tool name: calls, failures, time spent answering and bytes
//! returned. It exists to find the next expensive tool from what agents
//! actually call, rather than from a guess about which tool is heavy.
//!
//! ⚠️ BOUNDED BY NAME. A caller chooses the tool name, and an unknown name is
//! refused only after it is counted, so a client sending made-up names could
//! otherwise grow this without limit. At most [`CAPACITY`] names of at most
//! [`NAME_BYTES`] each are kept; calls to any further or longer name are
//! counted together under [`OVERFLOW`].
//!
//! Owned by the application state and lost on restart, which is why a reading
//! says when counting started.

use std::collections::BTreeMap;
use std::sync::{Mutex, PoisonError};

use serde::Serialize;

/// More names than the tool surface has, so every real tool keeps its own row.
pub(crate) const CAPACITY: usize = 160;
/// Longer than any real tool name, so a made-up name cannot carry a page of
/// text into the tally.
pub(crate) const NAME_BYTES: usize = 64;
/// Where calls to names beyond [`CAPACITY`] or longer than [`NAME_BYTES`] are
/// counted.
pub(crate) const OVERFLOW: &str = "(other names)";

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub(crate) struct ToolCost {
    pub calls: u64,
    /// Refused calls and answers marked as errors.
    pub failures: u64,
    pub total_micros: u64,
    pub max_micros: u64,
    /// The answer's text, which is what the calling agent reads. The same
    /// answer sent again as structured content is not counted (ADR 0145).
    pub total_bytes: u64,
    pub max_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct NamedToolCost {
    pub name: String,
    #[serde(flatten)]
    pub cost: ToolCost,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct AgentToolCostsReading {
    /// When this process started counting, in unix seconds.
    pub since: i64,
    pub observed_at: i64,
    /// Most time spent answering first.
    pub tools: Vec<NamedToolCost>,
}

#[derive(Debug)]
pub(crate) struct AgentToolCosts {
    since: i64,
    tools: Mutex<BTreeMap<String, ToolCost>>,
}

impl Default for AgentToolCosts {
    fn default() -> Self {
        Self {
            since: crate::unix_timestamp(),
            tools: Mutex::default(),
        }
    }
}

impl AgentToolCosts {
    pub(crate) fn record(&self, tool: &str, failed: bool, micros: u64, bytes: u64) {
        // A poisoned lock still holds plain counters, and a count is not worth
        // failing a tool call over.
        let mut tools = self.tools.lock().unwrap_or_else(PoisonError::into_inner);
        let name =
            if tool.len() <= NAME_BYTES && (tools.contains_key(tool) || tools.len() < CAPACITY) {
                tool
            } else {
                OVERFLOW
            };
        let cost = tools.entry(name.to_owned()).or_default();
        cost.calls = cost.calls.saturating_add(1);
        cost.failures = cost.failures.saturating_add(u64::from(failed));
        cost.total_micros = cost.total_micros.saturating_add(micros);
        cost.max_micros = cost.max_micros.max(micros);
        cost.total_bytes = cost.total_bytes.saturating_add(bytes);
        cost.max_bytes = cost.max_bytes.max(bytes);
    }

    pub(crate) fn reading(&self, now: i64) -> AgentToolCostsReading {
        let tools = self.tools.lock().unwrap_or_else(PoisonError::into_inner);
        let mut tools = tools
            .iter()
            .map(|(name, cost)| NamedToolCost {
                name: name.clone(),
                cost: *cost,
            })
            .collect::<Vec<_>>();
        tools.sort_by(|left, right| {
            right
                .cost
                .total_micros
                .cmp(&left.cost.total_micros)
                .then_with(|| left.name.cmp(&right.name))
        });
        AgentToolCostsReading {
            since: self.since,
            observed_at: now,
            tools,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tool_is_counted_by_calls_time_and_bytes_with_the_costliest_first() {
        let costs = AgentToolCosts::default();
        costs.record("swarm_list_tasks", false, 9_000, 40_000);
        costs.record("swarm_list_tasks", true, 1_000, 200);
        costs.record("swarm_record_task_note", false, 2_000, 300);

        let reading = costs.reading(costs.since + 60);

        assert_eq!(reading.tools[0].name, "swarm_list_tasks");
        assert_eq!(
            reading.tools[0].cost,
            ToolCost {
                calls: 2,
                failures: 1,
                total_micros: 10_000,
                max_micros: 9_000,
                total_bytes: 40_200,
                max_bytes: 40_000,
            }
        );
        assert_eq!(reading.tools[1].name, "swarm_record_task_note");
        assert_eq!(reading.observed_at, reading.since + 60);
    }

    /// The caller names the tool, so the tally has to stop growing somewhere.
    #[test]
    fn names_past_the_capacity_are_counted_together() {
        let costs = AgentToolCosts::default();
        for index in 0..CAPACITY + 25 {
            costs.record(&format!("made_up_{index}"), true, 1, 1);
        }
        costs.record("made_up_0", false, 1, 1);

        let reading = costs.reading(0);

        assert_eq!(reading.tools.len(), CAPACITY + 1);
        let overflow = reading
            .tools
            .iter()
            .find(|tool| tool.name == OVERFLOW)
            .expect("the overflow row exists");
        assert_eq!(overflow.cost.calls, 25);
        let first = reading
            .tools
            .iter()
            .find(|tool| tool.name == "made_up_0")
            .expect("a name seen before the bound keeps its row");
        assert_eq!(first.cost.calls, 2);
    }

    #[test]
    fn a_name_longer_than_any_tool_is_counted_with_the_others() {
        let costs = AgentToolCosts::default();
        let long = "x".repeat(NAME_BYTES + 1);
        costs.record(&long, true, 1, 1);
        costs.record(&"x".repeat(NAME_BYTES), true, 1, 1);

        let reading = costs.reading(0);

        assert!(reading.tools.iter().all(|tool| tool.name != long));
        let names = reading
            .tools
            .iter()
            .map(|tool| (tool.name.len(), tool.cost.calls))
            .collect::<Vec<_>>();
        assert!(names.contains(&(OVERFLOW.len(), 1)));
        assert!(names.contains(&(NAME_BYTES, 1)));
    }
}

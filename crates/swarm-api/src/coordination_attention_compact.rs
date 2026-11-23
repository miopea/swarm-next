//! Short rows over Queen's coordination attention, filtered, with what the
//! filters left out counted (ADR 0185).
//!
//! ⚠️ THE FULL RESPONSE WAS TOO LARGE TO READ. Sixteen sections of row-capped
//! detail came back as one answer, and Queen saved it to a file and ran jq over
//! it to find what waited on whom. This keeps one short row per item instead:
//! which task, whose move, how long, and why it waits. Each section's guidance
//! and every field stay in the full answer, which she asks for by section.
//!
//! ⚠️ A FILTERED ANSWER SAYS WHAT IT LEFT OUT. An empty filtered list reads as
//! "nothing waits" unless it says otherwise, so every section reports what was
//! shown, what the filters left out, and what the row cap left out.
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use swarm_domain::{NextMoveOwner, TaskState};

pub(crate) const DEFAULT_LIMIT_PER_SECTION: usize = 8;
pub(crate) const MAX_LIMIT_PER_SECTION: usize = 64;
/// The longest "why" a row carries, in characters.
const WHY_CHARS: usize = 160;

/// What Queen asked for. Every field is optional.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AttentionQuery {
    #[serde(default)]
    pub detail: Detail,
    /// Full only: which sections to return. Absent means all of them.
    #[serde(default)]
    pub sections: Vec<String>,
    #[serde(default)]
    pub state: Vec<TaskState>,
    #[serde(default)]
    pub owner: Vec<NextMoveOwner>,
    pub workspace: Option<String>,
    pub min_age_seconds: Option<i64>,
    pub limit_per_section: Option<usize>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Detail {
    #[default]
    Compact,
    Full,
}

impl AttentionQuery {
    fn filters_by_task(&self) -> bool {
        !self.state.is_empty() || !self.owner.is_empty() || self.workspace.is_some()
    }

    fn limit(&self) -> usize {
        self.limit_per_section
            .unwrap_or(DEFAULT_LIMIT_PER_SECTION)
            .clamp(1, MAX_LIMIT_PER_SECTION)
    }
}

/// The facts a row's task contributes: what the filters test and what the row
/// shows beside the section's own fields.
#[derive(Clone, Debug)]
pub(crate) struct TaskFacts {
    pub title: String,
    pub state: TaskState,
    pub owner: NextMoveOwner,
    pub workspace: String,
    pub updated_at: i64,
}

/// One section of the full answer and how its items become rows.
struct Section {
    key: &'static str,
    /// JSON pointer to the section's items: an array, or one object.
    items: &'static str,
    /// The item's own ids a follow-up tool needs rather than the task's. One
    /// field is shown as its value, several as an object of those present.
    reference: &'static [&'static str],
    /// The item's own title field, when it has one.
    title: Option<&'static str>,
    age: Age,
    why: fn(&Value) -> String,
    /// Whether the section itself stopped short of every item.
    truncated: fn(&Value) -> bool,
}

#[derive(Clone, Copy)]
enum Age {
    /// The item carries its age in seconds.
    Seconds(&'static str),
    /// The item carries the time it started waiting.
    Since(&'static str),
    /// Time since its task last changed.
    TaskUpdated,
}

fn text(item: &Value, field: &str) -> String {
    match &item[field] {
        Value::String(value) => value.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn flag(section: &Value, field: &str) -> bool {
    section[field].as_bool().unwrap_or(false)
}

fn never(_: &Value) -> bool {
    false
}

/// Every section of the full answer, in its order. A test holds this list to
/// the keys the full answer actually has, so a new section cannot be left out
/// of the compact answer unnoticed.
const SECTIONS: [Section; 16] = [
    Section {
        key: "unfinished_delivered_review",
        items: "/unfinished_delivered_review",
        reference: &["run_id"],
        title: None,
        age: Age::TaskUpdated,
        why: |_| {
            "Your automation review run is unfinished: finish it under this run_id with \
             swarm_finish_automation_run."
                .into()
        },
        truncated: never,
    },
    Section {
        key: "active_work_recovery",
        items: "/active_work_recovery/tasks",
        reference: &[],
        title: Some("task_title"),
        age: Age::TaskUpdated,
        why: |item| {
            format!(
                "{}: {}'s terminal is resting",
                text(item, "attention_kind"),
                text(item, "worker_name")
            )
        },
        truncated: never,
    },
    Section {
        key: "unexplained_briefing_observations",
        items: "/unexplained_briefing_observations/workers",
        reference: &[],
        title: None,
        age: Age::TaskUpdated,
        why: |item| {
            format!(
                "Its briefing to {} is held by something on that terminal",
                text(item, "worker_name")
            )
        },
        truncated: |all| flag(&all["unexplained_briefing_observations"], "truncated"),
    },
    Section {
        key: "pending_decision_overlaps",
        items: "/pending_decision_overlaps/groups",
        reference: &["decision_ids"],
        title: None,
        age: Age::TaskUpdated,
        why: |item| {
            format!(
                "{} pending decisions share this task",
                text(item, "pending_count")
            )
        },
        truncated: |all| flag(&all["pending_decision_overlaps"], "truncated"),
    },
    Section {
        key: "clarification_attention",
        items: "/clarification_attention/pending/requests",
        reference: &["decision_id"],
        title: None,
        age: Age::TaskUpdated,
        why: |_| {
            "The operator asked a question on this decision; answer it with \
             swarm_reply_clarification"
                .into()
        },
        truncated: |all| flag(&all["clarification_attention"]["pending"], "truncated"),
    },
    Section {
        key: "queue_snapshot",
        items: "/queue_snapshot/queen_tasks",
        reference: &[],
        title: Some("title"),
        age: Age::TaskUpdated,
        why: |item| format!("Yours to move ({})", text(item, "state")),
        truncated: |all| flag(&all["queue_snapshot"], "queen_tasks_truncated"),
    },
    Section {
        key: "blocked_reassessment",
        items: "/blocked_reassessment/tasks",
        reference: &[],
        title: Some("title"),
        age: Age::TaskUpdated,
        why: |item| format!("Blocked: {}", text(item, "recorded_block_excerpt")),
        truncated: |all| flag(&all["blocked_reassessment"], "truncated"),
    },
    Section {
        key: "prerequisite_ready",
        items: "/prerequisite_ready/tasks",
        reference: &[],
        title: Some("title"),
        age: Age::TaskUpdated,
        why: |_| "Its prerequisites are finished: resume it if nothing else blocks it".into(),
        truncated: |all| flag(&all["prerequisite_ready"], "truncated"),
    },
    Section {
        key: "attention",
        items: "/attention",
        reference: &["action_id"],
        title: Some("task_title"),
        age: Age::Seconds("age_seconds"),
        why: |item| format!("{}: {}", text(item, "kind"), text(item, "reason")),
        truncated: never,
    },
    Section {
        key: "task_message_deliveries",
        items: "/task_message_deliveries/items",
        reference: &["message_id", "claim_id"],
        title: Some("task_title"),
        age: Age::Seconds("waiting_seconds"),
        why: |item| {
            let held = &item["held_behind"]["title"];
            let reason = text(item, "resolution_reason");
            if held.is_string() {
                format!(
                    "Message to {} waits for its Active task \"{}\"",
                    text(item, "recipient_name"),
                    text(&item["held_behind"], "title")
                )
            } else if reason.is_empty() {
                format!("Message delivery {}", text(item, "state"))
            } else {
                format!("Message {}: {reason}", text(item, "state"))
            }
        },
        truncated: |all| {
            let messages = &all["task_message_deliveries"];
            let listed = messages["items"].as_array().map_or(0, Vec::len);
            messages["total"].as_u64().unwrap_or(0) > listed as u64
        },
    },
    Section {
        key: "reviews_repeating",
        items: "/reviews_repeating",
        reference: &[],
        title: Some("title"),
        age: Age::Since("first_seen_at"),
        why: |item| {
            format!(
                "Reviewed {} times without a block or a prerequisite",
                text(item, "times_seen")
            )
        },
        truncated: never,
    },
    Section {
        key: "recoveries_repeating",
        items: "/recoveries_repeating",
        reference: &[],
        title: Some("title"),
        age: Age::Since("first_seen_at"),
        why: |item| {
            format!(
                "The same recovery verdict {} times, with no progress",
                text(item, "times_seen")
            )
        },
        truncated: never,
    },
    Section {
        key: "unrouted_ready_work",
        items: "/unrouted_ready_work",
        reference: &[],
        title: Some("title"),
        age: Age::Seconds("waiting_seconds"),
        why: |_| "Ready with no worker".into(),
        truncated: never,
    },
    Section {
        key: "unasked_stalled_work",
        items: "/unasked_stalled_work",
        reference: &[],
        title: Some("title"),
        age: Age::Seconds("still_seconds"),
        why: |_| "Has not moved, and nobody has asked anyone about it".into(),
        truncated: never,
    },
    Section {
        key: "held_briefings",
        items: "/held_briefings",
        reference: &[],
        title: Some("title"),
        age: Age::Since("queued_at"),
        why: |item| {
            format!(
                "Briefing to {} held: {}",
                text(item, "worker_name"),
                text(item, "reason")
            )
        },
        truncated: never,
    },
    Section {
        key: "unreachable_assignments",
        items: "/unreachable_assignments",
        reference: &[],
        title: Some("title"),
        age: Age::Since("assigned_at"),
        why: |item| {
            format!(
                "Assigned to {}, which is not running",
                text(item, "worker_name")
            )
        },
        truncated: never,
    },
];

/// The section keys the compact answer reads, in order.
pub(crate) fn section_keys() -> Vec<&'static str> {
    SECTIONS.iter().map(|section| section.key).collect()
}

/// Refuses a section name that is not one, so a typo cannot read as an empty
/// section.
///
/// # Errors
/// Names the unknown section and lists the known ones.
pub(crate) fn check_sections(names: &[String]) -> Result<(), String> {
    let known = section_keys();
    match names.iter().find(|name| !known.contains(&name.as_str())) {
        Some(unknown) => Err(format!(
            "unknown section \"{unknown}\"; the sections are: {}",
            known.join(", ")
        )),
        None => Ok(()),
    }
}

/// The full answer, narrowed to the sections named when any are.
pub(crate) fn narrow_full(mut full: Value, names: &[String]) -> Value {
    if !names.is_empty()
        && let Some(object) = full.as_object_mut()
    {
        object.retain(|key, _| names.iter().any(|name| name == key));
    }
    full
}

/// The tasks the full answer's items are about.
pub(crate) fn task_ids(full: &Value) -> std::collections::BTreeSet<String> {
    SECTIONS
        .iter()
        .flat_map(|section| items(full, section.items))
        .filter_map(|item| item["task_id"].as_str().map(str::to_owned))
        .collect()
}

impl From<&swarm_domain::Task> for TaskFacts {
    fn from(task: &swarm_domain::Task) -> Self {
        Self {
            title: task.title.clone(),
            state: task.state,
            owner: task.next_move_owner,
            workspace: task.workspace.clone(),
            updated_at: task.updated_at,
        }
    }
}

fn items(full: &Value, pointer: &str) -> Vec<Value> {
    match full.pointer(pointer) {
        Some(Value::Array(items)) => items.clone(),
        Some(Value::Object(item)) => vec![Value::Object(item.clone())],
        _ => Vec::new(),
    }
}

fn excerpt(text: &str) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= WHY_CHARS {
        return flat;
    }
    let mut short: String = flat.chars().take(WHY_CHARS - 1).collect();
    short.push('…');
    short
}

/// The compact answer: short rows, the board's counts, and per section what
/// was shown and what was left out (ADR 0185).
pub(crate) fn compact(
    full: &Value,
    tasks: &HashMap<String, TaskFacts>,
    query: &AttentionQuery,
    now: i64,
) -> Value {
    let limit = query.limit();
    let mut rows = Vec::new();
    let mut summaries = Vec::new();
    let mut empty = Vec::new();
    let (mut left_by_filters, mut left_by_limit) = (0_usize, 0_usize);
    for section in &SECTIONS {
        if !query.sections.is_empty() && !query.sections.iter().any(|name| name == section.key) {
            continue;
        }
        let all = items(full, section.items);
        let source_truncated = (section.truncated)(full);
        if all.is_empty() && !source_truncated {
            empty.push(section.key);
            continue;
        }
        let (mut shown, mut filtered, mut capped) = (0_usize, 0_usize, 0_usize);
        for item in &all {
            let task_id = item["task_id"].as_str();
            let facts = task_id.and_then(|id| tasks.get(id));
            let age = match section.age {
                Age::Seconds(field) => item[field].as_i64(),
                Age::Since(field) => item[field].as_i64().map(|since| now - since),
                Age::TaskUpdated => facts.map(|task| now - task.updated_at),
            };
            if !matches(query, facts, age) {
                filtered += 1;
                continue;
            }
            if shown == limit {
                capped += 1;
                continue;
            }
            shown += 1;
            rows.push(row(section, item, task_id, facts, age));
        }
        left_by_filters += filtered;
        left_by_limit += capped;
        summaries.push(json!({
            "section": section.key,
            "shown": shown,
            "left_out_by_filters": filtered,
            "left_out_by_limit": capped,
            "source_truncated": source_truncated,
        }));
    }
    json!({
        "detail": "compact",
        "filters": {
            "state": query.state,
            "owner": query.owner,
            "workspace": query.workspace,
            "min_age_seconds": query.min_age_seconds,
            "limit_per_section": limit,
        },
        "board": {
            "open_tasks": full["queue_snapshot"]["open_tasks"],
            "by_state": full["queue_snapshot"]["by_state"],
            "by_owner": full["queue_snapshot"]["by_owner"],
        },
        "rows": rows,
        "sections": summaries,
        "empty_sections": empty,
        "left_out": {"by_filters": left_by_filters, "by_limit": left_by_limit},
        "note": COMPACT_NOTE,
    })
}

const COMPACT_NOTE: &str = "Compact rows (ADR 0185): one per item, with whose move it is, how long it has waited and why. Before acting on a row, read its section in full with detail \"full\" and sections [its section]: the guidance for that section and every field are there. A section's left_out counts say what the filters and the row cap did not show, and source_truncated says the section itself holds more than it returned; neither means nothing waits. A row with no task is left out by any state, owner or workspace filter.";

fn matches(query: &AttentionQuery, facts: Option<&TaskFacts>, age: Option<i64>) -> bool {
    if query.filters_by_task() {
        let Some(task) = facts else {
            return false;
        };
        if !query.state.is_empty() && !query.state.contains(&task.state) {
            return false;
        }
        if !query.owner.is_empty() && !query.owner.contains(&task.owner) {
            return false;
        }
        if query
            .workspace
            .as_deref()
            .is_some_and(|workspace| workspace != task.workspace)
        {
            return false;
        }
    }
    match query.min_age_seconds {
        Some(minimum) => age.is_some_and(|age| age >= minimum),
        None => true,
    }
}

fn row(
    section: &Section,
    item: &Value,
    task_id: Option<&str>,
    facts: Option<&TaskFacts>,
    age: Option<i64>,
) -> Value {
    let mut row = Map::new();
    row.insert("section".into(), json!(section.key));
    if let Some(task_id) = task_id {
        row.insert("task_id".into(), json!(task_id));
    }
    let present: Map<String, Value> = section
        .reference
        .iter()
        .filter(|field| !item[**field].is_null())
        .map(|field| ((*field).to_owned(), item[*field].clone()))
        .collect();
    match (section.reference, present.values().next()) {
        (_, None) => {}
        ([_], Some(value)) => {
            row.insert("ref".into(), value.clone());
        }
        _ => {
            row.insert("ref".into(), Value::Object(present));
        }
    }
    let title = section
        .title
        .map(|field| text(item, field))
        .filter(|title| !title.is_empty())
        .or_else(|| facts.map(|task| task.title.clone()));
    if let Some(title) = title {
        row.insert("title".into(), json!(title));
    }
    if let Some(task) = facts {
        row.insert("state".into(), json!(task.state));
        row.insert("owner".into(), json!(task.owner));
    }
    if let Some(age) = age {
        row.insert("age_seconds".into(), json!(age));
    }
    row.insert("why".into(), json!(excerpt(&(section.why)(item))));
    Value::Object(row)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(state: TaskState, owner: NextMoveOwner, workspace: &str) -> TaskFacts {
        TaskFacts {
            title: "From the board".into(),
            state,
            owner,
            workspace: workspace.into(),
            updated_at: 1_000,
        }
    }

    fn full() -> Value {
        json!({
            "unfinished_delivered_review": {"run_id": "run-1", "delivery_session_id": "s"},
            "queue_snapshot": {
                "open_tasks": 3,
                "by_state": [{"state": "review", "count": 2}],
                "by_owner": [{"owner": "queen", "count": 2}],
                "queen_tasks": [
                    {"task_id": "a", "title": "Judge A", "state": "review"},
                    {"task_id": "b", "title": "Judge B", "state": "review"},
                ],
                "queen_tasks_truncated": false,
            },
            "blocked_reassessment": {"tasks": [], "truncated": true},
            "attention": [{
                "action_id": "act-1", "kind": "stale_owned_work_attention",
                "task_id": "c", "task_title": "Stale C",
                "reason": "x ".repeat(200), "age_seconds": 7_200,
            }],
            "task_message_deliveries": {"items": [], "total": 0},
            "unasked_stalled_work": [
                {"task_id": "a", "title": "Judge A", "still_seconds": 90_000},
            ],
        })
    }

    fn board() -> HashMap<String, TaskFacts> {
        HashMap::from([
            (
                "a".into(),
                facts(TaskState::Review, NextMoveOwner::Queen, "/repo/one"),
            ),
            (
                "b".into(),
                facts(TaskState::Review, NextMoveOwner::Queen, "/repo/two"),
            ),
            (
                "c".into(),
                facts(TaskState::Active, NextMoveOwner::Worker, "/repo/one"),
            ),
        ])
    }

    fn rows(answer: &Value) -> Vec<(String, String)> {
        answer["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| {
                (
                    row["section"].as_str().unwrap().to_owned(),
                    row["task_id"].as_str().unwrap_or("").to_owned(),
                )
            })
            .collect()
    }

    fn section<'a>(answer: &'a Value, key: &str) -> &'a Value {
        answer["sections"]
            .as_array()
            .unwrap()
            .iter()
            .find(|summary| summary["section"] == key)
            .unwrap()
    }

    /// Every item becomes one short row with whose move it is, its age and why.
    #[test]
    fn every_item_becomes_one_short_row() {
        let answer = compact(&full(), &board(), &AttentionQuery::default(), 10_000);
        assert_eq!(
            rows(&answer),
            [
                ("unfinished_delivered_review".into(), String::new()),
                ("queue_snapshot".into(), "a".into()),
                ("queue_snapshot".into(), "b".into()),
                ("attention".into(), "c".into()),
                ("unasked_stalled_work".into(), "a".into()),
            ]
        );
        let review = &answer["rows"][0];
        assert_eq!(review["ref"], "run-1");
        assert!(review.get("age_seconds").is_none());
        let stale = &answer["rows"][3];
        assert_eq!(stale["ref"], "act-1");
        assert_eq!(stale["title"], "Stale C");
        assert_eq!(stale["state"], "active");
        assert_eq!(stale["owner"], "worker");
        assert_eq!(stale["age_seconds"], 7_200);
        let why = stale["why"].as_str().unwrap();
        assert_eq!(why.chars().count(), WHY_CHARS, "{why}");
        assert!(why.starts_with("stale_owned_work_attention: x x"), "{why}");
        assert_eq!(answer["rows"][1]["age_seconds"], 9_000);
        assert_eq!(answer["rows"][4]["age_seconds"], 90_000);
        assert_eq!(answer["board"]["open_tasks"], 3);
        assert!(
            answer["empty_sections"]
                .as_array()
                .unwrap()
                .contains(&json!("held_briefings"))
        );
    }

    /// A filter says what it left out, so an empty answer is not "nothing
    /// waits"; a row with no task is left out by a task filter, and counted.
    #[test]
    fn a_filter_counts_what_it_left_out() {
        let query = AttentionQuery {
            owner: vec![NextMoveOwner::Queen],
            workspace: Some("/repo/one".into()),
            ..AttentionQuery::default()
        };
        let answer = compact(&full(), &board(), &query, 10_000);
        assert_eq!(
            rows(&answer),
            [
                ("queue_snapshot".into(), "a".into()),
                ("unasked_stalled_work".into(), "a".into()),
            ]
        );
        assert_eq!(
            section(&answer, "unfinished_delivered_review")["left_out_by_filters"],
            1
        );
        assert_eq!(section(&answer, "queue_snapshot")["left_out_by_filters"], 1);
        assert_eq!(section(&answer, "attention")["left_out_by_filters"], 1);
        assert_eq!(answer["left_out"]["by_filters"], 3);

        let old = AttentionQuery {
            min_age_seconds: Some(10_000),
            state: vec![TaskState::Review],
            ..AttentionQuery::default()
        };
        let answer = compact(&full(), &board(), &old, 10_000);
        assert_eq!(rows(&answer), [("unasked_stalled_work".into(), "a".into())]);
        assert_eq!(answer["left_out"]["by_filters"], 4);
    }

    /// The row cap is counted too, and a section that stopped short at its
    /// source says so even when it returned nothing.
    #[test]
    fn the_cap_and_a_truncated_source_are_said() {
        let query = AttentionQuery {
            limit_per_section: Some(1),
            ..AttentionQuery::default()
        };
        let answer = compact(&full(), &board(), &query, 10_000);
        let queen = section(&answer, "queue_snapshot");
        assert_eq!(queen["shown"], 1);
        assert_eq!(queen["left_out_by_limit"], 1);
        assert_eq!(answer["left_out"]["by_limit"], 1);
        let blocked = section(&answer, "blocked_reassessment");
        assert_eq!(blocked["shown"], 0);
        assert_eq!(blocked["source_truncated"], true);
        assert_eq!(
            AttentionQuery {
                limit_per_section: Some(10_000),
                ..AttentionQuery::default()
            }
            .limit(),
            MAX_LIMIT_PER_SECTION
        );
    }

    /// A message row carries what reconciling it needs, and leaves out a claim
    /// it never had (ADR 0184).
    #[test]
    fn a_message_row_carries_its_ids() {
        let full = json!({"task_message_deliveries": {"total": 2, "items": [
            {"message_id": "m1", "claim_id": "c1", "task_id": "a", "task_title": "Judge A",
             "state": "uncertain", "waiting_seconds": 60},
            {"message_id": "m2", "claim_id": null, "task_id": "a", "task_title": "Judge A",
             "state": "rejected", "waiting_seconds": 30,
             "resolution_reason": "its task closed before it arrived"},
        ]}});
        let answer = compact(&full, &board(), &AttentionQuery::default(), 10_000);
        assert_eq!(
            answer["rows"][0]["ref"],
            json!({"message_id": "m1", "claim_id": "c1"})
        );
        assert_eq!(answer["rows"][1]["ref"], json!({"message_id": "m2"}));
        assert_eq!(
            answer["rows"][1]["why"],
            "Message rejected: its task closed before it arrived"
        );
        assert_eq!(answer["rows"][0]["why"], "Message delivery uncertain");
    }

    /// The answer narrows to the sections named, and a name that is not a
    /// section is refused rather than read as an empty one.
    #[test]
    fn sections_narrow_the_answer_and_an_unknown_one_is_refused() {
        let narrowed = narrow_full(full(), &["attention".into()]);
        assert_eq!(
            narrowed.as_object().unwrap().keys().collect::<Vec<_>>(),
            ["attention"]
        );
        assert_eq!(narrow_full(full(), &[]), full());
        let refused = check_sections(&["atention".into()]).unwrap_err();
        assert!(refused.contains("\"atention\""), "{refused}");
        assert!(refused.contains("task_message_deliveries"), "{refused}");
        assert!(check_sections(&["attention".into()]).is_ok());

        let query = AttentionQuery {
            sections: vec!["attention".into()],
            ..AttentionQuery::default()
        };
        let answer = compact(&full(), &board(), &query, 10_000);
        assert_eq!(rows(&answer), [("attention".into(), "c".into())]);
        assert_eq!(answer["sections"].as_array().unwrap().len(), 1);
        assert!(
            answer["empty_sections"].as_array().unwrap().is_empty(),
            "a section not asked for is not called empty"
        );
        assert_eq!(
            answer["board"]["open_tasks"], 3,
            "the board is always counted"
        );
    }
}

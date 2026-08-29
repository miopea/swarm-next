import type { BlockedEscalation, DecisionRequest, HeldBriefing, Task, Worker } from "../api";

/**
 * A Hive that never existed, for pictures that can be published.
 *
 * WHY INVENTED DATA HERE, when docs/38 says to transcribe what the operator
 * actually saw. That rule is for surfaces being DEBUGGED — inventing plausible
 * data there renders a screen nobody has looked at, which is the whole point of
 * looking. These fixtures are for the opposite job: a picture that goes into a
 * public README. Every name here is deliberately fictional, because the
 * alternative is anonymising a live Hive and hoping the scrubbing held.
 *
 * The 0.9.2 captures this replaces were made that way — DOM mutation over a
 * real Hive — and the terminal could not be scrubbed at all, because it is a
 * canvas. Sessions inspected during that work held the operator's banking
 * institutions. Fixtures are safe by construction rather than by inspection.
 */
const now = Math.floor(Date.now() / 1000);

function task(
  id: string,
  title: string,
  description: string,
  state: Task["state"],
  workspace: string,
  worker: string | null,
  position: number,
  priority: Task["priority"] = "normal",
): Task {
  return {
    id,
    hive_id: "0199ffff-0000-7000-8000-000000000000",
    title,
    description,
    operator_instruction: "",
    priority,
    workspace,
    state,
    assigned_worker_id: worker,
    assigned_session_id: null,
    position,
    created_at: now - 86_400 * 2,
    updated_at: now - 3_600,
    worked_here: true,
  };
}

export const demoWorkers: Worker[] = [
  {
    id: "0199aaaa-0000-7000-8000-000000000001",
    hive_id: "0199ffff-0000-7000-8000-000000000000",
    name: "Queen",
    role: "queen",
    provider: "claude_code",
    workspace: "/home/you/projects/orchard",
    autostart: true,
    position: 0,
    active_session_id: "session-queen",
    created_at: now - 86_400 * 30,
    updated_at: now - 60,
    running: true,
    attention_state: "buzzing",
    description: "Always-active command terminal",
  },
  {
    id: "0199aaaa-0000-7000-8000-000000000002",
    hive_id: "0199ffff-0000-7000-8000-000000000000",
    name: "Orchard Web",
    role: "worker",
    provider: "claude_code",
    workspace: "/home/you/projects/orchard-web",
    autostart: false,
    position: 1,
    active_session_id: "session-web",
    created_at: now - 86_400 * 20,
    updated_at: now - 120,
    running: true,
    attention_state: "buzzing",
  },
  {
    id: "0199aaaa-0000-7000-8000-000000000003",
    hive_id: "0199ffff-0000-7000-8000-000000000000",
    name: "Orchard API",
    role: "worker",
    provider: "claude_code",
    workspace: "/home/you/projects/orchard-api",
    autostart: false,
    position: 2,
    active_session_id: null,
    created_at: now - 86_400 * 18,
    updated_at: now - 900,
    running: false,
    attention_state: "resting",
  },
  {
    id: "0199aaaa-0000-7000-8000-000000000004",
    hive_id: "0199ffff-0000-7000-8000-000000000000",
    name: "Field Notes",
    role: "worker",
    provider: "codex",
    workspace: "/home/you/projects/field-notes",
    autostart: false,
    position: 3,
    active_session_id: null,
    created_at: now - 86_400 * 9,
    updated_at: now - 1_800,
    running: false,
    attention_state: "sleeping",
  },
];

export const demoTasks: Task[] = [
  // A finished task nobody showed to be live. Present so the harness can render
  // the "Waiting on evidence" panel at all -- the panel the operator reported,
  // and the one the unverifiable control lives in.
  {
    ...task(
      "0199bbbb-0000-7000-8000-00000000000f",
      "Send the launch digest on the new schedule",
      "Shipped by a worker whose session has long ended. Nothing recorded where it went.",
      "completed",
      "/home/you/projects/field-notes",
      "0199aaaa-0000-7000-8000-000000000004",
      5,
    ),
    closed_on_evidence: false,
    deployment_recorded: false,
  },
  task(
    "0199bbbb-0000-7000-8000-000000000001",
    "Signed-out visitors see the pricing page instead of a redirect loop",
    "Reported twice this week. The guard runs before the session is read, so an anonymous visit bounces between /login and /pricing until the browser gives up.",
    "active",
    "/home/you/projects/orchard-web",
    "0199aaaa-0000-7000-8000-000000000002",
    0,
  ),
  task(
    "0199bbbb-0000-7000-8000-000000000002",
    "Nightly export finishes but writes an empty file when the source is slow",
    "The writer closes on a timeout and reports success. Nothing downstream can tell an empty export from a quiet day.",
    "ready",
    "/home/you/projects/orchard-api",
    "0199aaaa-0000-7000-8000-000000000003",
    1,
    "high",
  ),
  task(
    "0199bbbb-0000-7000-8000-000000000003",
    "Give the search index a way to say it is stale",
    "It currently answers confidently from whatever it last built. A reader cannot tell a fresh answer from a week-old one.",
    "ready",
    "/home/you/projects/orchard-api",
    null,
    2,
  ),
  task(
    "0199bbbb-0000-7000-8000-000000000004",
    "Field notes lose their attachment when the upload is retried",
    "The retry reuses the draft id but not the attachment reference, so the second attempt saves the note without the photo.",
    "review",
    "/home/you/projects/field-notes",
    "0199aaaa-0000-7000-8000-000000000004",
    3,
  ),
  task(
    "0199bbbb-0000-7000-8000-000000000005",
    "Write the upgrade note for the 2.0 config change",
    "Anyone upgrading has to move two settings by hand. Say which, and what happens if they do not.",
    "draft",
    "/home/you/projects/orchard",
    null,
    4,
    "low",
  ),
];

export const demoDecision: DecisionRequest = {
  id: "demo-decision-1",
  hive_id: "demo-hive",
  requesting_worker_id: "demo-orchard-api",
  task_id: "demo-task-export",
  kind: "input",
  urgency: "normal",
  title: "Should a slow source fail the export, or write what it has?",
  summary:
    "The nightly export can finish before the source has answered. Failing loudly means a missed night; writing a partial file means nobody downstream can tell it is partial. Both are recoverable, and by different people, so this is yours rather than mine.",
  reason:
    "I can implement either in about the same time. I am asking because the wrong choice is silent: a partial export looks exactly like a quiet day.",
  risk: "",
  evidence: "",
  suggested_action: "Write what arrived and mark it partial",
  allowed_actions: ["Fail the run and alert", "Write what arrived and mark it partial"],
  deadline: null,
  state: "pending",
  resolution_action: null,
  resolution_note: "",
  resolved_by_operator_id: null,
  created_at: now - 5_400,
  updated_at: now - 5_400,
  resolved_at: null,
  delivery_state: "delivered",
};

export const demoBlocked: BlockedEscalation[] = [
  {
    task_id: "demo-task-index",
    title: "Give the search index a way to say it is stale",
    worker_name: "Orchard API",
    workspace: "/home/you/projects/orchard-api",
    blocked_for_seconds: 14 * 3600,
  },
];

export const demoBriefings: HeldBriefing[] = [
  ["Retry the upload before giving up on the attachment", "Field Notes", 2_100],
  ["Say which settings moved in the 2.0 config change", "Field Notes", 900],
].map(([title, worker, age], index) => ({
  task_id: `demo-briefing-${index}`,
  title: title as string,
  worker_id: `demo-worker-${index}`,
  worker_name: worker as string,
  queued_at: now - (age as number),
  reason: "worker_already_working",
  blocked_by: null,
}));


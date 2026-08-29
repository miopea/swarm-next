import type { Task, Worker } from "../api";

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

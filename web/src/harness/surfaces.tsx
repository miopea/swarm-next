import type { ReactNode } from "react";
import TerminalQuestionsFixture from "./TerminalQuestionsFixture";
import TerminalBurstFixture from "./TerminalBurstFixture";
import TerminalView from "../terminal/TerminalView";
import { MobileTerminalComposer } from "../terminal/MobileTerminalComposer";
import DeveloperDogfoodWorkspace from "../settings/DeveloperDogfoodWorkspace";
import DatabaseRecoveryCard from "../runtime/DatabaseRecoveryCard";
import NightWatchSettings from "../settings/NightWatchSettings";
import TerminalPoolFixture from "./TerminalPoolFixture";
import PerformanceEvidenceFixture from "./PerformanceEvidenceFixture";

import type { BlockedEscalation, Connection, DecisionRequest, HeldBriefing, UnansweredEmailTask } from "../api";
import { App } from "../App";
import { SURFACE_STORAGE_KEY } from "../navigation/startSurface";
import QueuesView from "../queues/QueuesView";
import TaskBoard from "../tasks/TaskBoard";
import TaskPrerequisiteDialog from "../tasks/TaskPrerequisiteDialog";
import WorkerRosterItem from "../workers/WorkerRosterItem";
import WorkerSettings from "../settings/WorkerSettings";
import { demoBlocked, demoBriefings, demoDecision, demoTasks, demoWorkers } from "./productFixtures";
import DecisionInbox from "../decisions/DecisionInbox";
import UnansweredEmailAttentionCard from "../tasks/UnansweredEmailAttentionCard";
import ConnectionsSettings from "../settings/ConnectionsSettings";
import MachinePressureBadge from "../runtime/MachinePressureBadge";
import StaleBundleNotice from "../StaleBundleNotice";
import WorkerContextBar from "../workers/WorkerContextBar";
import WhatsNewModal from "../runtime/WhatsNewModal";
import UnsettledReviewCard from "../decisions/UnsettledReviewCard";
import { machinePressureNotice, type MachineResourceState } from "../runtime/machinePressure";

/**
 * The fixtures are the operator's ACTUAL screenshot, transcribed.
 *
 * Inventing plausible data would render a surface nobody has ever seen. The
 * point of looking is to look at what they looked at, so the counts, the
 * repeated worker names and the waiting times come from 01a04943's screenshot.
 */
const now = Math.floor(Date.now() / 1000);

const waitingEmail: UnansweredEmailTask = {
  task_id: "01a0492a-5966-72f3-8b60-68a5b395d494",
  title: "It does not seem like worker's state is being updated in mobile on the picker (+2 related)",
  sender_name: "Bradford Schleifer",
  sender_address: "bschleifer@rcg.org",
  received_at: now - 340_000,
  drafted: true,
  sending: false,
  draft_id: "draft-1",
  draft_body: [
    "Both are sorted, though the picker one wasn't quite what it looked like.",
    "",
    "The mobile picker was drawing every worker with the same default bee, and the Queen as an ordinary worker — so it looked frozen even when the state underneath was current. It now uses the same component as the desktop roster, so the two can't drift apart again.",
    "",
    "The \"busy but showing as resting\" one was fixed the same evening you wrote in. A worker that had finished and one that still had something running both said \"Resting\". It now says \"Resting · task running\" when something is still going.",
    "",
    "Both are live on the Hive your phone uses, and in 0.9.1 and 0.9.2.",
    "",
    "One thing I haven't done: I couldn't find any case where the state genuinely stopped updating, so if it still looks stale on your phone, tell me and I'll dig again.",
  ].join("\n"),
  worker_name: "Swarm Next",
  thread_count: 3,
  delivery_failure: null,
};

const briefings: HeldBriefing[] = [
  ["Watch face generator: procedural texture engines", "BFG Watchfaces", 2_460],
  ["Watch face app: detect and select the target watch; scope My Faces to the device", "BFG Watchfaces", 2_460],
  ["Watch face app: complication spacing should control both axes", "BFG Watchfaces", 2_460],
  ["Watch face app: the localhost demo is the spec for the shipped APK", "BFG Watchfaces", 2_460],
  ["MCP: let the coach correct a logged set's load, the way the History correction form already can", "Sculpt Studio", 1_260],
  ["Watch face app: rewrite all UI copy for end users, not developers", "BFG Watchfaces", 1_020],
  ["Plates show on the rest card but not the active card; RPE grid overflows the correction form", "Sculpt Studio", 900],
].map(([title, worker, age], index) => ({
  task_id: `briefing-${index}`,
  title: title as string,
  worker_id: `worker-${worker as string}`,
  worker_name: worker as string,
  queued_at: now - (age as number),
  reason: "worker_already_working",
  blocked_by: null,
}));

const queenInput: DecisionRequest = {
  id: "decision-1",
  hive_id: "hive-1",
  requesting_worker_id: "queen",
  task_id: "task-plates",
  kind: "input",
  urgency: "normal",
  title: "Plate breakdown and the RPE grid — the interview you asked for",
  summary: "You said to interview you on the three workout screenshots. I read all three first: plates render on the rest card and not on the active card, which explains both plate emails at once. The RPE overflow is separate. Four questions, none of which I can answer for you.",
  reason: "",
  risk: "",
  evidence: "Screenshot_20260828-084330.png — Rest card, Barbell hip thrust, 0:40.",
  suggested_action: "Answer the four questions",
  allowed_actions: ["Answer the four questions"],
  deadline: null,
  state: "pending",
  resolution_action: null,
  resolution_note: "",
  resolved_by_operator_id: null,
  created_at: now - 600,
  updated_at: now - 600,
  resolved_at: null,
  delivery_state: "delivered",
};


/**
 * The four machine states, side by side.
 *
 * Waiting for a real box to run out of memory is not a way to check what the
 * warning looks like, and the Normal case is the ONLY one an operator ever
 * sees by accident — so the three that matter had never been looked at. The
 * numbers are a plausible 32 GiB developer machine at each threshold from
 * crates/swarm-api/src/runtime.rs: advisory at 85% used or PSI 2, critical at
 * 95% or PSI 10.
 */
const machineFixture = (
  pressure: "normal" | "advisory" | "critical" | "unavailable",
  used: number | null,
  psi: number | null,
  load: number,
) => ({
  kind: "ready" as const,
  resources: {
    sampled_at: Math.floor(Date.now() / 1000),
    policy: { mode: "observe_only", advisory_percent: 85, critical_percent: 95 },
    api: { resident_memory_bytes: 92 * 1024 * 1024, pressure: "normal" },
    terminal_host: { resident_memory_bytes: 48 * 1024 * 1024, pressure: "normal" },
    machine: {
      memory_total_bytes: 32 * 1024 ** 3,
      memory_available_bytes: 3 * 1024 ** 3,
      memory_used_percent: used,
      swap_total_bytes: 8 * 1024 ** 3,
      swap_used_bytes: 3 * 1024 ** 3,
      swap_used_percent: 37.5,
      load_average: [load, load - 0.4, load - 0.9] as [number, number, number],
      logical_cpus: 4,
      memory_pressure_avg10: psi,
      cpu_pressure_avg10: 1.1,
      io_pressure_avg10: 0.4,
      pressure,
    },
  },
}) as MachineResourceState;

const MACHINE_STATES: { name: string; state: MachineResourceState }[] = [
  { name: "Normal — 41% used, no stall. The header must stay silent.", state: machineFixture("normal", 41, 0.1, 1.2) },
  { name: "Advisory — 88% used, stall 3.2%. Warns BEFORE the crash.", state: machineFixture("advisory", 88, 3.2, 7.3) },
  { name: "Critical — 96% used, stall 14.8%. The box is about to go.", state: machineFixture("critical", 96, 14.8, 22.6) },
  { name: "Unavailable — the machine block came back with nothing readable.", state: machineFixture("unavailable", null, null, 0) },
  { name: "Failed — the resource request itself did not come back.", state: { kind: "failed" } },
  { name: "Loading — silent on purpose, so it does not flash on every page load.", state: { kind: "loading" } },
];

/**
 * The Connections card, against fixtures rather than a Hive.
 *
 * It is given its loader, because it reads for itself. The failed-read state is
 * the one most worth looking at — an empty list is the reassuring answer and
 * would be a lie — and no real API response can produce it.
 */
const connectedTools: Connection[] = [
  {
    id: "01a04a00-0000-7000-8000-000000000001",
    name: "Claude Desktop",
    connected_at: now - 86_400 * 3,
    last_seen_at: now - 900,
  },
  {
    id: "01a04a00-0000-7000-8000-000000000002",
    name: "VS Code",
    connected_at: now - 86_400 * 11,
    last_seen_at: now - 86_400 * 6,
  },
];




/**
 * The card that escaped the measure.
 *
 * It is in the fixture BECAUSE it escaped: the earlier fix capped four card
 * classes by name and this one was not among them, so it rendered full width
 * beside content capped at 880 and the operator saw three widths on one screen.
 * A surface that renders only the cards someone remembered cannot catch that.
 */
const agedBlock: BlockedEscalation[] = [
  {
    task_id: "01a04a00-0000-7000-8000-0000000000aa",
    title: "Add FLEET_PR_TOKEN so sync-propagate can open its PR — and run the success path for the first time",
    worker_name: "Codex Shared Config",
    workspace: "/workspace/codex-shared-config",
    blocked_for_seconds: 13 * 3600,
  },
];

/**
 * Invented content for the picture that goes in the README.
 *
 * The `needs-you` fixture above is transcribed from the operator's real screen
 * on purpose — that is what makes it useful for DEBUGGING, and exactly what
 * makes it unpublishable. It carries their name, a real reply, real project
 * names and a real credential name. Two surfaces, two jobs.
 */
/** Every callback a surface needs and a picture never uses. */
const noop = () => undefined;
const asyncNoop = async () => undefined;

export type Surface = { id: string; title: string; why: string; render: () => ReactNode };

/** Filed times for the unsettled fixture, spread the way the real rows are. */
const FILED = Math.floor(Date.now() / 1000) - 7 * 24 * 60 * 60;

export const SURFACES: Surface[] = [
  {
    id: "composer-source", title: "Composer source recording failure", why: "Synthetic narrow layout; no terminal or API writes.",
    render: () => <main className="composer-source-fixture" style={{ maxWidth: 390, padding: 16 }}>
      <style>{".composer-source-fixture .mobile-terminal-composer { display: block; }"}</style>
      <h1>Composer source fixture</h1>
      <p>Isolated narrow layout. Send simulates accepted input and failed source recording; no worker receives text.</p>
      <MobileTerminalComposer connectionState="connected" onInput={() => true} onRecordSubmission={async () => { throw new Error("fixture unavailable"); }} />
    </main>,
  },
  {
    id: "terminal-recovery", title: "Terminal recovery outcome", why: "Synthetic saved recovery result; no provider or Hive.",
    render: () => <main style={{ padding: 16, height: "100dvh", display: "flex", flexDirection: "column" }}>
      <h1>Recovery result fixture</h1>
      <p>Synthetic recovery evidence. Not a live provider test.</p>
      <div style={{ flex: 1, minHeight: 0 }}><TerminalView busy={false} operatorToken="fixture-only" session={{
        session_id: "fixture-terminal-recovery", running: true,
        confirmed_selection: new URLSearchParams(location.search).get("confirmed") === "1"
          ? { revision: 2, conversation: "fixture-later-selected-conversation" } : undefined,
        recovery_outcome: new URLSearchParams(location.search).get("outcome") === "restored"
          ? { state: "restored", conversation: "fixture-restored-conversation", via_continue: true }
          : { state: "manual", reason: "unexpected_conversation" },
      }} /></div>
    </main>,
  },
  { id: "performance-evidence", title: "Browser and server performance evidence", why: "Synthetic simultaneous, stale and unavailable evidence; no live profiling", render: () => <PerformanceEvidenceFixture /> },
  { id: "terminal-burst", title: "Terminal burst application", why: "Bounded synthetic packets through the real parser; no live workers", render: () => <TerminalBurstFixture /> },
  { id: "night-watch", title: "Night Watch schedule", why: "isolated schedule editor; saves are fixture responses only", render: () => <section className="settings-card"><NightWatchSettings operatorToken="fixture" /></section> },
  {
    id: "terminal-pool",
    title: "Terminal renderer pool lifecycle",
    why: "Eight synthetic workers for cold-return evidence wiring; not a production performance benchmark.",
    render: () => <TerminalPoolFixture />,
  },
  {
    id: "developer-dogfood",
    title: "Developer Dogfood",
    why: "Isolated diagnostics and opt-in warm-pool controls; no live workers or Hive requests.",
    render: () => <DeveloperDogfoodWorkspace operatorToken="fixture-only" runtime={{ enabled: true, version: "fixture-dev", state: "idle", reload_available: false, source_revision: "fixture-revision", source_dirty: false, deployed_source_published: false }} version="fixture-dev" reachable />,
  },
  {
    id: "terminal-handoff",
    title: "Terminal Resume Here",
    why: "real terminal UI over synthetic v4 messages; add &terminalControl=elsewhere for a passive view. No engine or provider is exercised",
    render: () => <main style={{ padding: 16, height: "100dvh", display: "flex", flexDirection: "column" }}>
      <h1>Terminal handoff fixture</h1>
      <p>Invented terminal. No Hive, worker, or provider. This tests browser wiring, not engine ownership.</p>
      <div style={{ flex: 1, minHeight: 0 }}><TerminalView session={{ session_id: "fixture-terminal-handoff", running: true }} operatorToken="fixture-only" busy={false} /></div>
    </main>,
  },
  {
    id: "terminal-questions",
    title: "Terminal question transitions",
    why: "synthetic narrow/wide ANSI repaint and snapshot recovery; not a captured Claude AskUser trace",
    render: () => <TerminalQuestionsFixture />,
  },
  {
    id: "unsettled-review",
    title: "Finished work nothing has settled",
    why: "eleven real rows, the count and title lengths the operator was actually looking at when they said 'no clear which worker ... I cannot scan it to know what i needed'. The three-row fixture this replaces had short titles and could not have produced any of the three faults",
    render: () => (
      <UnsettledReviewCard
        onOpenTask={() => {}}
        /*
         * THE SHAPE IS TRANSCRIBED FROM THE FAILING CASE; THE WORDS ARE NOT.
         *
         * Everything that produces the symptom is copied from the eleven rows
         * on the operator's Hive: eleven rows, six workers distributed 3/2/2/2/1/1,
         * reasons repeating 7/2/2, and title lengths that dominate the real ones
         * rank for rank — the longest is the real 172 characters. So this cannot
         * be a fixture chosen to pass: every dimension that made the card fail is
         * present at full strength.
         *
         * The TEXT is invented, and that is not squeamishness. This repository is
         * public. The real titles name client projects, an unreleased migration,
         * and one unfixed auth-redirect defect in a named production app — docs/38
         * lists "client and project names" among the things a published capture
         * must not carry, and a harness capture is meant to be safe by
         * construction rather than by inspection. Git makes that permanent.
         */
        waiting={[
          { task_id: "u1", title: "C2: app shell — no basePath, assetPrefix, app/[area] with the area validated as bud|leaf", workspace: "/workspace/orchard", worker_name: "Field Notes", kind: "code_no_deployment", reason: "it recorded commits that touch code, and no deployment", created_at: FILED + 0 },
          { task_id: "u2", title: "C3: middleware normalization — a status mismatch redirects, never 404s", workspace: "/workspace/orchard", worker_name: "Field Notes", kind: "code_no_deployment", reason: "it recorded commits that touch code, and no deployment", created_at: FILED + 14 },
          { task_id: "u3", title: "A2b: orchard-web — one mobile menu control, not two (split from A2, which fixed Meadow only)", workspace: "/workspace/orchard", worker_name: "Hedgerow", kind: "claim_unapproved", reason: "a claim that nothing was deployed, which nobody has approved", created_at: FILED + 1679 },
          { task_id: "u4", title: "A10c: orchard-web — five bare redirect('/login') calls; the client path is already correct", workspace: "/workspace/orchard", worker_name: "Hedgerow", kind: "claim_unapproved", reason: "a claim that nothing was deployed, which nobody has approved", created_at: FILED + 2231 },
          { task_id: "u5", title: "A10d: Meadow — save controls send ?next= but the sign-in screen only reads ?redirect=, so the return path is silently discarded", workspace: "/workspace/orchard", worker_name: "Meadow", kind: "nothing_reported", reason: "nobody reported what this work produced", created_at: FILED + 2250 },
          { task_id: "u6", title: "A13: retire the legacy tblNavigation sync — 83 dead rows that actively mislead", workspace: "/workspace/orchard", worker_name: "Orchard API", kind: "claim_unapproved", reason: "a claim that nothing was deployed, which nobody has approved", created_at: FILED - 56 },
          { task_id: "u7", title: "B6: nav catalog rows for the target IA, with role security entries", workspace: "/workspace/orchard", worker_name: "Orchard API", kind: "claim_unapproved", reason: "a claim that nothing was deployed, which nobody has approved", created_at: FILED + 0 },
          { task_id: "u8", title: "Reserve a native user id range so POST /api/users stops drawing from the mirrored legacy sequence — migration, orchard-data owns it", workspace: "/workspace/orchard", worker_name: "Orchard API", kind: "claim_unapproved", reason: "a claim that nothing was deployed, which nobody has approved", created_at: FILED + 5300 },
          { task_id: "u9", title: "C4: gateway route prefixes for /bud and /leaf", workspace: "/workspace/orchard", worker_name: "Orchard Web", kind: "claim_unapproved", reason: "a claim that nothing was deployed, which nobody has approved", created_at: FILED + 28 },
          { task_id: "u10", title: "A5c: orchard-web — a media-progress producer the original A5 brief missed entirely", workspace: "/workspace/orchard", worker_name: "Orchard Web", kind: "claim_unapproved", reason: "a claim that nothing was deployed, which nobody has approved", created_at: FILED + 2340 },
          { task_id: "u11", title: "A transition note reaches task history but not the worker's briefing — two workers confirmed it independently, and an undelivered instruction is indistinguishable from none", workspace: "/workspace/orchard", worker_name: "Windfall", kind: "nothing_reported", reason: "nobody reported what this work produced", created_at: FILED + 7521 },
        ]}
      />
    ),
  },
  {
    id: "whats-new",
    title: "What's New, at the width it actually renders",
    why: "1.0.0 printed its bullets' `**bold**` as literal asterisks, and the 760px width rule had been dead since it was written -- .dialog is defined later in the stylesheet and won on order",
    render: () => (
      <WhatsNewModal
        onDismiss={() => {}}
        releases={[{
          version: "1.0.0",
          notes: [
            // Bold at the START of a bullet: this is the shape 1.0.0 shipped,
            // and the shape that also has to survive the sentence capital.
            { summary: "**Feedback goes to GitHub.** The feedback dialog now files a real issue on the repository, and tells you where it went — including when it could not get there, instead of silently keeping it local", kind: "feat", needs_worker_engine_update: false },
            { summary: "**From 0.8.x — every worker session ends.** You are on the older terminal protocol, and the app and the worker engine have to be swapped together. The command is the same; there is nothing extra to run", kind: "feat", needs_worker_engine_update: false },
            // Code, and an identifier an italic rule would have eaten.
            { summary: "Check which you are on with `cat ~/.local/lib/swarm/current/VERSION` before installing", kind: "feat", needs_worker_engine_update: true },
            { summary: "A store failure is reported as what it was — the blanket \"temporarily unavailable\" that swallowed every underlying error in email_reply_deliveries now records the actual cause", kind: "fix", needs_worker_engine_update: false },
            { summary: "**A page load stops replaying the whole event history.** Opening the control room on a phone read thousands of entries to show the newest sixteen", kind: "fix", needs_worker_engine_update: false },
          ],
        }]}
        earlier={[
          { version: "1.0.0", notes: [{ summary: "**Feedback goes to GitHub.** The dialog files a real issue", kind: "feat", needs_worker_engine_update: false }] },
          { version: "0.9.2", notes: [{ summary: "An update confirms Swarm has actually stopped before replacing it", kind: "fix", needs_worker_engine_update: false }] },
        ]}
      />
    ),
  },
  {
    id: "worker-row",
    title: "The worker row at phone width",
    why: "the operator ruled it down to the name and Work here; everything else competed with them across a 390px screen",
    render: () => (
      // The real header trigger and the real context bar, in the classes the
      // product puts them in, so the mobile rules under test actually apply.
      <div className="workspace-header">
        <div><p className="eyebrow">Persistent terminal</p><div className="workspace-heading"><h2>BFG Watchfaces</h2></div></div>
        {/* BOTH CASES, because the line has two SOURCES even though it is now
            one element: the task a worker is carrying, and the repository it
            works in when it is carrying nothing. Measuring one of them is a
            check that cannot fail for the other, which is how the last
            regression shipped — the fixture only ever rendered the task. */}
        <button className="mobile-worker-switcher-trigger" type="button" aria-haspopup="dialog" aria-label="Switch worker, current BFG Watchfaces">
          <span className="worker-avatar" />
          <span>
            <span className="mobile-worker-task">Build the catalog service: anonymous submit, moderation queue</span>
            <strong>BFG Watchfaces</strong>
          </span>
          <span aria-hidden="true">⌄</span>
        </button>

        <WorkerContextBar
          worker={demoWorkers[0]}
          currentTask={demoTasks[0]}
          openCount={3}
          workSummary="1 active · 2 ready"
          repository={{ branch: "main", detached: false, changed_paths: 2 }}
          engagement={{ deviceClass: "desktop", detail: "another desktop is driving this worker" }}
          onClaim={() => {}}
          taskStateLabel={(task) => task.state}
          onOpenQueue={() => {}}
        />
      </div>
    ),
  },
  {
    id: "worker-row-no-task",
    title: "The worker row with no task in flight",
    why: "the second line falls back to the repository name; measuring only the task case is how the last regression shipped",
    render: () => (
      <div className="workspace-header">
        <div><p className="eyebrow">Persistent terminal</p><div className="workspace-heading"><h2>BudgetBug</h2></div></div>
        <button className="mobile-worker-switcher-trigger" type="button" aria-haspopup="dialog" aria-label="Switch worker, current BudgetBug">
          <span className="worker-avatar" />
          <span>
            <span className="mobile-worker-task">budgetbug</span>
            <strong>BudgetBug</strong>
          </span>
          <span aria-hidden="true">⌄</span>
        </button>
      </div>
    ),
  },
  {
    id: "nav-panels",
    title: "Rail panels in the mobile nav",
    why: "below 680px the nav is a 5-column grid and these panels are grid children, so each was laid out in one column — a fifth of the screen",
    render: () => (
      // The real nav container, with the class the product puts on it when the
      // Apiary tab is present, because that is the 5-column case. Buttons stand
      // in for the tabs so the grid has the columns it really has.
      <nav className="surface-nav with-apiary" aria-label="Harness nav">
        <button type="button"><span>Needs you</span></button>
        <button type="button"><span>Tasks</span></button>
        <button type="button" className="selected"><span>Workers</span></button>
        <button type="button"><span>Apiary</span></button>
        <button type="button"><span>Settings</span></button>
        <StaleBundleNotice
          stale
          serverVersion="0.9.2-dev-df05c92f7378-20260829191026-560886"
          dismissed={null}
          onDismiss={() => {}}
        />
      </nav>
    ),
  },
  {
    id: "needs-you",
    title: "Needs you",
    why: "an email draft and actionable request; waiting work is on the separate Queues fixture",
    render: () => (
      <div className="attention-workspace">
        <DecisionInbox
          decisions={[queenInput]}
          tasks={[]}
          workers={[]}
          busy={false}
          additionalPendingCount={1}
          attentionCards={<>
            <UnansweredEmailAttentionCard awaiting={[waitingEmail]} busy={false} onOpenTask={() => undefined} />
          </>}
          onResolve={async () => undefined}
        />
      </div>
    ),
  },
  {
    id: "queues",
    title: "Queues",
    why: "waiting work from the operator screenshot, separate from actionable Needs You requests; not publishable",
    render: () => <QueuesView tasks={[]} workers={[]} blockedWaits={agedBlock} heldBriefings={briefings} heldDeliveries={[
      { kind: "delivery_held_unsent_text", subject: "queen-review", worker_name: null, reason: "The last observed prompt contained unsent text.", first_observed_at: 1_787_402_241, observations: 172 },
    ]} onOpenTask={() => undefined} />,
  },
  {
    id: "machine-pressure",
    title: "Machine pressure",
    why: "the header badge in every state — the five an operator would only otherwise see by actually running their box out of memory",
    render: () => (
      <div className="harness-machine-states">
        {MACHINE_STATES.map(({ name, state }) => {
          const notice = machinePressureNotice(state);
          return (
            <div key={name} className="harness-machine-state">
              <p className="harness-machine-caption">{name}</p>
              {/* The real rail footer, so the badge is measured beside the
                  runtime line it actually sits next to rather than floating
                  alone on a blank page. */}
              <div className="rail-footer">
                <span className="runtime-status"><span className="presence online" /> Runtime 0.9.2</span>
                <MachinePressureBadge notice={notice} />
                {notice ? null : <em className="harness-machine-silent">(nothing rendered)</em>}
              </div>
            </div>
          );
        })}
      </div>
    ),
  },
  {
    id: "connections",
    title: "Connections",
    why: "the outside-tools card in all three states — connected, empty, and the read that failed",
    render: () => (
      <div className="harness-machine-states">
        <div className="harness-machine-state">
          <p className="harness-machine-caption">Two tools connected.</p>
          <ConnectionsSettings operatorToken="harness" load={async () => connectedTools} />
        </div>
        <div className="harness-machine-state">
          <p className="harness-machine-caption">Nothing connected — says how to connect one.</p>
          <ConnectionsSettings operatorToken="harness" load={async () => []} />
        </div>
        <div className="harness-machine-state">
          <p className="harness-machine-caption">
            The read failed. Must NOT read as &ldquo;nothing is connected&rdquo;.
          </p>
          <ConnectionsSettings operatorToken="harness" load={async () => { throw new Error("offline"); }} />
        </div>
      </div>
    ),
  },
  {
    id: "needs-you-demo",
    title: "Needs you (publishable)",
    why: "the same surface against INVENTED data — the needs-you fixture is transcribed from the operator's real screen and must never be published",
    render: () => (
      <div className="attention-workspace">
        <DecisionInbox
          decisions={[demoDecision]}
          tasks={demoTasks}
          workers={demoWorkers}
          busy={false}
          onResolve={async () => undefined}
        />
      </div>
    ),
  },
  {
    id: "needs-you-withdrawn",
    title: "Withdrawn request history",
    why: "An obsolete request is quiet and never represented as operator approval",
    render: () => <DecisionInbox decisions={[{
      ...demoDecision, state: "withdrawn", title: "Export recovery needed a decision",
      withdrawal_reason: "The source recovered and the verified export completed. No operator judgment is needed.",
      withdrawn_by_worker_id: demoWorkers[0].id, withdrawn_at: demoDecision.updated_at,
    }]} tasks={demoTasks} workers={demoWorkers} busy={false} onResolve={async () => undefined} />,
  },
  {
    id: "queues-demo",
    title: "Queues (publishable)",
    why: "invented task, blocked-age and briefing evidence; no operator data",
    render: () => <QueuesView tasks={demoTasks} workers={demoWorkers} blockedWaits={demoBlocked} heldBriefings={[
      ...demoBriefings,
      { task_id: demoTasks[2].id, title: demoTasks[2].title, worker_id: demoTasks[2].assigned_worker_id!,
        worker_name: "Orchard API", queued_at: now - 120, reason: "waiting_its_turn", blocked_by: "Verify the export schema" },
    ]} onOpenTask={() => undefined} />,
  },
  {
    id: "prerequisite-editor",
    title: "Prerequisite editor",
    why: "synthetic operator form; no live task mutations",
    render: () => <TaskPrerequisiteDialog task={demoTasks[3]} candidates={demoTasks} operatorToken="harness" onChanged={noop} onClose={noop} />,
  },
  {
    id: "prerequisite-editor-phone",
    title: "Phone prerequisite editor",
    why: "390px layout only, not native mobile acceptance",
    render: () => <iframe title="Phone prerequisite editor" src="/harness.html?surface=prerequisite-editor" style={{ display: "block", width: 390, height: 844, border: 0 }} />,
  },
  {
    id: "queues-prerequisites",
    title: "Explicit task prerequisites",
    why: "fictional blocked, completed and reopened prerequisite states; layout only",
    render: () => <QueuesView workers={demoWorkers} onOpenTask={() => undefined} tasks={[
      { ...demoTasks[3], prerequisites: [{ task_id: demoTasks[3].id, prerequisite_id: demoTasks[2].id, title: "Publish the shared API response contract before the mobile stale-state indicator can choose its fallback behavior", state: "active", assigned_worker_id: demoWorkers[2].id, removed: false, reason: "The API worker needs to settle the response shape before this worker implements its view.", created_at: now - 300 }] },
      { ...demoTasks[3], id: "ready-for-queen", title: "Use the completed API contract in the search view", next_move_owner: "queen", prerequisites: [{ task_id: "ready-for-queen", prerequisite_id: demoTasks[2].id, title: "Publish the shared API response contract", state: "completed", assigned_worker_id: demoWorkers[2].id, removed: false, reason: "Contract first", created_at: now - 300 }] },
      { ...demoTasks[1], prerequisites: [{ task_id: demoTasks[1].id, prerequisite_id: demoTasks[2].id, title: "Confirm the signed-out request contract", state: "completed", assigned_worker_id: demoWorkers[2].id, removed: true, reason: "The previous contract task was removed and needs reconciliation.", created_at: now - 300 }] },
    ]} />,
  },
  {
    id: "queues-prerequisites-phone",
    title: "Phone prerequisite layout",
    why: "the prerequisite surface in a real 390px frame, not native mobile acceptance",
    render: () => <iframe title="Phone prerequisite preview" src="/harness.html?surface=queues-prerequisites" style={{ display: "block", width: 390, height: 844, border: 0 }} />,
  },
  {
    id: "app",
    title: "The whole control room",
    why: "the REAL App against fixtures — rail, header and content in one frame. Add &screen=decisions|tasks|workers to pick the screen",
    render: () => {
      // The app reads ?surface= too, and the harness has already claimed it.
      // A second parameter picks the screen, seeded through the storage the
      // app itself reads so nothing in production has to know.
      const screen = new URLSearchParams(window.location.search).get("screen");
      if (screen) {
        try {
          window.sessionStorage.setItem(SURFACE_STORAGE_KEY, screen);
        } catch {
          // Private windows refuse storage; the app opens on its default.
        }
      }
      return <App />;
    },
  },
  {
    id: "queues-phone",
    title: "Phone Queues layout",
    why: "the full fictional App in a real 390px frame viewport; not a native PWA/device test",
    render: () => <iframe title="Phone Queues preview" src="/harness.html?surface=app&screen=queues"
      style={{ display: "block", width: 390, height: 844, border: 0 }} />,
  },
  {
    id: "needs-you-phone",
    title: "Phone Needs you layout",
    why: "the full fictional App in a real 390px frame viewport; not a native PWA/device test",
    render: () => <iframe title="Phone Needs you preview" src="/harness.html?surface=app&screen=decisions"
      style={{ display: "block", width: 390, height: 844, border: 0 }} />,
  },
  {
    id: "database-recovery",
    title: "Database recovery attention",
    why: "synthetic failure notice rendered without reading or changing a database",
    render: () => <div style={{ maxWidth: 390 }}><DatabaseRecoveryCard /></div>,
  },
  {
    id: "tasks",
    title: "Tasks",
    why: "the board, against invented work — the source for the README screenshot",
    render: () => (
      <TaskBoard
        tasks={demoTasks}
        jiraTaskLinks={[]}
        operatorToken="harness"
        sessions={[]}
        workers={demoWorkers}
        busy={false}
        onCreate={asyncNoop}
        onUpdate={asyncNoop}
        onRemove={asyncNoop}
        onRestore={asyncNoop}
        onTransition={asyncNoop}
        onAssign={asyncNoop}
        onStartWorker={asyncNoop}
        onOpenWorker={noop}
        onFetchActivity={async () => ({ events: [], truncated: false })}
        onFetchJiraComments={async () => []}
        onAddJiraComment={async () => ({ state: "queued" })}
        onRetryJira={asyncNoop}
        onJiraImported={asyncNoop}
        onReorder={asyncNoop}
      />
    ),
  },
  {
    id: "worker-order",
    title: "Worker order",
    why: "the reorder list, which NOBODY BUT THE OPERATOR could see. Two of the three UI defects reported on 2026-09-01 were invisible to tests because the screen carrying them had no surface here; this is the third, and it is the screen where a 409 that refuses EVERY submission looks identical to a drag that did not register.",
    render: () => (
      <WorkerSettings
        workers={demoWorkers}
        workspaces={[]}
        busy={false}
        providers={{ claude_code: true, codex: true }}
        onCreate={asyncNoop}
        onUpdate={asyncNoop}
        onChooseMark={asyncNoop}
        onRemove={asyncNoop}
        onDraftDescription={async () => "A drafted description."}
        onImproveDescription={async () => "An improved description."}
        onReorder={asyncNoop}
      />
    ),
  },
  {
    id: "worker-experimental",
    title: "Existing experimental worker",
    why: "Preserve the actual provider while editing an existing worker; never display it as Claude.",
    render: () => <WorkerSettings workers={[{ ...demoWorkers[1], provider: "gemini", running: false }]}
      workspaces={[]} busy={false} providers={{ claude_code: true, codex: true }}
      onCreate={asyncNoop} onUpdate={asyncNoop} onChooseMark={asyncNoop} onRemove={asyncNoop}
      onDraftDescription={async () => "Fixture description"} onReorder={asyncNoop} />,
  },
  {
    id: "workers",
    title: "Workers",
    why: "the roster, in its four attention states — the terminal itself is a canvas and is never captured",
    render: () => (
      <div className="harness-roster">
        {demoWorkers.map((worker, index) => (
          <WorkerRosterItem
            key={worker.id}
            worker={worker}
            selected={index === 0}
            detail={worker.workspace.split("/").pop() ?? ""}
            workSummary={index === 1 ? "1 active · 2 ready" : undefined}
            busy={false}
            onOpen={noop}
            onStart={noop}
            onStop={noop}
          />
        ))}
      </div>
    ),
  },
];

import type { ReactNode } from "react";

import type { BlockedEscalation, Connection, DecisionRequest, HeldBriefing, UnansweredEmailTask } from "../api";
import { App } from "../App";
import { SURFACE_STORAGE_KEY } from "../navigation/startSurface";
import BlockedEscalationCard from "../decisions/BlockedEscalationCard";
import TaskBoard from "../tasks/TaskBoard";
import WorkerRosterItem from "../workers/WorkerRosterItem";
import { demoBlocked, demoBriefings, demoDecision, demoTasks, demoWorkers } from "./productFixtures";
import DecisionInbox from "../decisions/DecisionInbox";
import HeldBriefingList from "../orchestration/HeldBriefingList";
import UnansweredEmailAttentionCard from "../tasks/UnansweredEmailAttentionCard";
import ConnectionsSettings from "../settings/ConnectionsSettings";
import MachinePressureBadge from "../runtime/MachinePressureBadge";
import StaleBundleNotice from "../StaleBundleNotice";
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

export const SURFACES: Surface[] = [
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
    why: "the composition cb1ffd5 changed — an email draft, queued briefings and a request that blocks work",
    render: () => (
      <div className="attention-workspace">
        <DecisionInbox
          decisions={[queenInput]}
          tasks={[]}
          workers={[]}
          busy={false}
          additionalPendingCount={1}
          attentionCards={<>
            <BlockedEscalationCard escalations={agedBlock} />
            <UnansweredEmailAttentionCard awaiting={[waitingEmail]} busy={false} onOpenTask={() => undefined} />
          </>}
          trailingCards={<HeldBriefingList briefings={briefings} />}
          onResolve={async () => undefined}
        />
      </div>
    ),
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
          tasks={[]}
          workers={[]}
          busy={false}
          attentionCards={<BlockedEscalationCard escalations={demoBlocked} />}
          trailingCards={<HeldBriefingList briefings={demoBriefings} />}
          onResolve={async () => undefined}
        />
      </div>
    ),
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

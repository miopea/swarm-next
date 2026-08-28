import type { ReactNode } from "react";

import type { DecisionRequest, HeldBriefing, UnansweredEmailTask } from "../api";
import DecisionInbox from "../decisions/DecisionInbox";
import HeldBriefingList from "../orchestration/HeldBriefingList";
import UnansweredEmailAttentionCard from "../tasks/UnansweredEmailAttentionCard";

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

export type Surface = { id: string; title: string; why: string; render: () => ReactNode };

export const SURFACES: Surface[] = [
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
          attentionCards={<UnansweredEmailAttentionCard awaiting={[waitingEmail]} busy={false} onOpenTask={() => undefined} />}
          trailingCards={<HeldBriefingList briefings={briefings} />}
          onResolve={async () => undefined}
        />
      </div>
    ),
  },
];

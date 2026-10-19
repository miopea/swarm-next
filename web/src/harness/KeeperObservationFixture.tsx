import { useEffect, useRef, useState } from "react";
import KeeperControlRoom from "../apiary/KeeperControlRoom";
import MemberControlRoom from "../apiary/MemberControlRoom";
import type { HiveIdentity } from "../api";

const identity = { operator: { id: "fictional", display_name: "Bea" }, hive: { id: "fictional", name: "Meadow Hive", operator_id: "fictional", apiary_id: "garden" }, apiary_context: { mode: "federated", local_role: "keeper", apiary: { id: "garden", name: "Meadow Test Garden", keeper_operator_id: "fictional", shared_work_backend: "jira" } } } satisfies HiveIdentity;

/** No-proxy fixture only: all reads are fictional and no action reaches a Hive. */
export default function KeeperObservationFixture({ member = false }: { member?: boolean }) {
  const mode = useRef<"failed" | "ready">("failed");
  const [mounted, setMounted] = useState(false);
  useEffect(() => {
    const previous = window.fetch;
    window.fetch = async (input, init) => {
      if (mode.current === "failed") throw new Error("Fictional observation failure");
      const url = String(input);
      const now = Math.floor(Date.now() / 1000);
      const line = (sequence: number, secondsAgo: number, level: string, target: string, message: string) => ({ sequence, at_ms: (now - secondsAgo) * 1000, level, target, message });
      // Opening a watch answers with the watch, as the real route does, so the
      // window knows which Hive it is waiting on.
      if (url.endsWith("/apiary/watches") && init?.method === "POST") {
        const target = (JSON.parse(String(init.body)) as { target_hive_id: string }).target_hive_id;
        return new Response(JSON.stringify({ id: "fictional-watch", target_hive_id: target, state: "requested", requested_at: now, acknowledged_at: null, expires_at: now + 300, ended_at: null }), { headers: { "Content-Type": "application/json" } });
      }
      const payload = url.endsWith("/policy-settings") ? {
          revision: 3,
          settings: [{ key: "release.auto_install", value: "on" }, { key: "shared_work.minimum_version", value: "1.16.10" }, { key: "queen.autonomy.night_watch", value: "advisory" }, { key: "night_watch.hours", value: "22:00-07:00" }],
          supported: [
            { key: "release.auto_install", kind: "choice", values: ["on", "off"] },
            { key: "shared_work.minimum_version", kind: "version", values: [] },
            ...(["at_hive", "away", "night_watch"] as const).map((tier) => ({ key: `queen.autonomy.${tier}`, kind: "choice", values: ["advisory", "coordinate", "local_execution"] })),
            { key: "night_watch.hours", kind: "hours", values: [] },
            { key: "checks.gate", kind: "choice", values: ["off", "warn", "require"] },
            { key: "worker.model", kind: "choice", values: ["default", "opus", "sonnet", "haiku"] },
            { key: "terminal.history_days", kind: "choice", values: ["1", "3", "7", "14", "30"] },
          ],
        }
        // Thistle's release is too old for either; the others comply and say nothing.
        // Clover is reachable but stopping at a step and short of disk; Thistle
        // is quiet, which the version and contact line already says.
        : url.endsWith("/fleet-health") ? [
          { hive_id: "clover", conditions: [
            { kind: "failing_sync", step: "project catalog", last_success_at: now - 2 * 3_600, sustained: true },
            { kind: "low_disk", lowest_available_bytes: 1_610_612_736, critical: false },
            { kind: "engine_update_pending" },
            { kind: "ci_unreadable", why: "gh is not logged in on this machine" },
            { kind: "workers_off_model", count: 2, model: "sonnet" },
          ] },
          { hive_id: "thistle", conditions: [{ kind: "quiet", last_contact_at: now - 3 * 86_400 }, { kind: "schema_behind", running: "1.11.0" }] },
        ]
        : url.includes("/apiary/usage") ? (() => {
          const totals = (weighted: number, active_seconds: number) => ({ input_tokens: 0, cache_write_tokens: 0, cache_read_tokens: 0, output_tokens: 0, messages: 0, weighted, active_seconds });
          return {
            days: url.endsWith("days=30") ? 30 : 7,
            hives: [
              { hive_id: "fictional", is_local: true, received_at: null, totals: totals(763_000_000, 41 * 3_600), by_model: [], by_workspace: [
                { name: "/home/you/projects/queen", ...totals(512_000_000, 30 * 3_600) },
                { name: "/home/you/projects/orchard-api", ...totals(251_000_000, 11 * 3_600) },
              ] },
              { hive_id: "clover", is_local: false, received_at: now - 240, totals: totals(248_000_000, 19 * 3_600 + 20 * 60), by_model: [], by_workspace: [
                { name: "/home/cora/projects/sculpt-studio-with-a-long-repository-name", ...totals(248_000_000, 19 * 3_600 + 20 * 60) },
              ] },
              { hive_id: "heather", is_local: false, received_at: null, totals: totals(0, 0), by_model: [], by_workspace: [] },
            ],
            totals: totals(1_011_000_000, 60 * 3_600 + 20 * 60),
            by_model: [{ name: "claude-opus-5-5", ...totals(804_000_000, 0) }, { name: "claude-sonnet-5-5", ...totals(207_000_000, 0) }],
          };
        })()
        // A morning without DNS, as WSL had on 2026-10-01.
        : url.includes("/contact-history") ? (() => {
          const until = now - (now % 300);
          const from = until - 86_400 + 300;
          const outage = [from + 120 * 300, from + 158 * 300];
          return { slot_seconds: 300, from, until, contacted: Array.from({ length: 288 }, (_, index) => from + index * 300).filter((at) => at < outage[0] || at >= outage[1]) };
        })()
        : url.endsWith("/policy-gaps") ? [{ hive_id: "thistle", gaps: [
          { key: "release.auto_install", expected: "on", reason: "release_cannot_apply" },
          { key: "shared_work.minimum_version", expected: "1.16.10", reason: "below_minimum", running: "1.11.0" },
          { key: "queen.autonomy.night_watch", expected: "advisory", reason: "release_cannot_apply" },
          { key: "night_watch.hours", expected: "22:00-07:00", reason: "release_cannot_apply" },
        ] }, { hive_id: "clover", gaps: [
          { key: "night_watch.hours", expected: "22:00-07:00", reason: "needs_local_choice" },
          { key: "terminal.history_days", expected: "3", reason: "engine_update_pending" },
        ] }, { hive_id: "bramble", gaps: [
          // Hidden behind its expired membership, which is the cause (ADR 0117).
          { key: "release.auto_install", expected: "on", reason: "not_reported" },
        ] }]
        : url.endsWith("/required-settings") ? {
          settings: [{ key: "release.auto_install", value: "on" }, { key: "shared_work.minimum_version", value: "1.16.10" }, { key: "queen.autonomy.night_watch", value: "advisory" }, { key: "night_watch.hours", value: "22:00-07:00" }],
          shared_work_refusal: "this Hive runs 1.16.9; the Apiary needs 1.16.10 or newer to take shared work, so update it",
        }
        : url.endsWith("/diagnostics/log") ? { entries: [line(1, 90, "info", "swarm_api", "Swarm API listening address=127.0.0.1:8766")], dropped: 0, received_at: null }
        : url.endsWith("/diagnostics") ? {
            entries: [
              line(40, 300, "info", "swarm_api", "Swarm API listening address=127.0.0.1:8766"),
              line(41, 240, "warn", "swarm_api", "Apiary synchronisation stopped at this step; it retries on a backoff step=\"project catalog\" condition=Incompatible"),
              line(42, 180, "debug", "swarm_api::takeover_producer", "takeover relay connected to Keeper lease=01a0eee4-2874-7491-85c3-5bc21d940346 revision=2"),
              line(43, 120, "warn", "swarm_api::takeover_producer", "the takeover holder's keystrokes were refused lease=01a0eee4-2874-7491-85c3-5bc21d940346 reason=terminal_operation_failed: this terminal requires generation-bound control"),
              line(44, 60, "error", "swarm_api", "the terminal host did not answer error=connection refused"),
            ],
            dropped: 2, received_at: now - 20,
          }
        : url.endsWith("/catalog-readiness") ? { acknowledgement: null, jira_connection: "not_connected", projects: [], blockers: [] }
        // Stopped at one step and waiting out its backoff: the state a field
        // member sat in for days while saying only "needs attention".
        : url.endsWith("/sync-health") ? { condition: "incompatible", last_attempt_at: now - 20, last_success_at: now - 3 * 86_400, consecutive_failures: 4, next_attempt_at: now + 100, failed_step: "project catalog" }
        : url.endsWith("/task-sync-status") ? { cursor: 0, task_count: 0 }
        : url.endsWith("/task-outbox-status") ? { queued_count: 0, conflict_count: 0, rejected_count: 0 }
        : url.endsWith("/my-stewardship") ? null
        : url.endsWith("/steward/assists") ? { incoming: [], outbox: [] }
        // ⚠️ THIS USED TO RETURN THE LOCAL HIVE ALONE, so the roster's action
        // row — Watch, Take over, and the version line beside them — never
        // rendered here at all. That is why the surface built to let somebody
        // LOOK at this page could not have caught the clipped button shipped in
        // 1.13.0. A roster fixture with nothing to act on is a fixture of a
        // different page. Remote Hives now appear, in the standings an operator
        // has to be able to tell apart at a glance.
        : url.endsWith("/members")
        ? [
            { hive_id: "fictional", hive_name: "Meadow Hive", operator_id: "fictional", operator_display_name: "Bea", role: "keeper", is_local: true },
            { hive_id: "clover", hive_name: "Clover Hive", operator_id: "operator-2", operator_display_name: "Cora", operator_email: "cora@example.invalid", role: "member", is_local: false, last_contact_at: now - 25 },
            // Silent for days while still listing the release it last reported —
            // how a Hive already updated went on reading as behind.
            { hive_id: "thistle", hive_name: "Thistle Hive", operator_id: "operator-3", operator_display_name: "Wren", operator_email: "wren@example.invalid", role: "member", is_local: false, last_contact_at: now - 3 * 86_400 },
            // Its membership lapses in eight days and it has not renewed (ADR 0117).
            { hive_id: "heather", hive_name: "Heather Hive", operator_id: "operator-4", operator_display_name: "Fen", role: "member", is_local: false, credential_expires_at: now + 8 * 86_400 },
            // Joined, never reached the Keeper, and its membership expired: the
            // roster used to say only that it "hasn't reported".
            { hive_id: "bramble", hive_name: "Bramble Hive", operator_id: "operator-5", operator_display_name: "Rue", operator_email: "rue@example.invalid", role: "member", is_local: false, credential_expires_at: now - 3 * 86_400 },
          ]
        : url.endsWith("/fleet-versions")
        ? {
            expected_release: "1.13.1", expected_release_first_seen_at: 100, expected_schema_version: 192,
            hives: [
              { hive_id: "fictional", node_id: "node-1", swarm_version: "1.13.1", database_schema_version: 192, observed_at: 100, standing: "current", raises: false, reports_task_waits: true },
              { hive_id: "clover", node_id: "node-2", swarm_version: "1.13.1", database_schema_version: 192, observed_at: 100, standing: "current", raises: false, reports_task_waits: true },
              { hive_id: "thistle", node_id: "node-3", swarm_version: "1.11.0", database_schema_version: 188, observed_at: 100, standing: "schema_behind", raises: true, reports_task_waits: false },
              // Heather reports no version at all, which is the case that has to
              // read as "unknown" rather than as an empty gap in the row.
            ],
          }
        // Shared work waiting three ways (ADR 0119): on an outside party, on a
        // member's operator, and on a Hive too old to say why.
        : url.endsWith("/apiary/tasks") ? (() => {
          const shared = (id: string, title: string, home: string, state: string, wait?: object) => ({ id, apiary_id: "garden", source: "swarm", title, description: "", priority: "normal", state, home_node_id: `node-${home}`, home_hive_id: home, revision: 4, created_at: now - 86_400, updated_at: now - 600, ...(wait ? { wait } : {}) });
          return [
            shared("shared-outside", "Move the office onto the backup circuit", "clover", "blocked", { kind: "outside", needs: "The carrier restores the backup circuit to Available", party: "the carrier", resume_at: now + 2 * 86_400 }),
            shared("shared-operator", "Turn on second-step sign-in for the shared mailbox", "clover", "blocked", { kind: "operator_action", needs: "Sign in to the identity portal and approve the second step", hive_id: "clover" }),
            shared("shared-silent", "Publish the watch face privacy policy", "thistle", "blocked"),
            shared("shared-active", "Rotate the shared signing key", "clover", "active"),
          ];
        })()
        : [];
      return new Response(JSON.stringify(payload), { headers: { "Content-Type": "application/json" } });
    };
    setMounted(true);
    return () => { window.fetch = previous; };
  }, []);
  return <>
    <p>Fictional {member ? "Member" : "Keeper"} observations. These controls cannot reach a Hive.</p>
    <button type="button" onClick={() => { mode.current = "ready"; document.dispatchEvent(new Event("visibilitychange")); }}>Restore fictional reads</button>
    <button type="button" onClick={() => { mode.current = "failed"; document.dispatchEvent(new Event("visibilitychange")); }}>Fail fictional reads</button>
    {mounted ? member ? <MemberControlRoom identity={{ ...identity, apiary_context: { ...identity.apiary_context, local_role: "member" } }} operatorToken="fictional" onManage={() => {}} onOpenTasks={() => {}} /> : <KeeperControlRoom identity={identity} operatorToken="fictional" onManage={() => {}} onInvite={() => {}} onOpenTasks={() => {}} /> : null}
  </>;
}

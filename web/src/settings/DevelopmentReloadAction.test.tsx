import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import DevelopmentReloadAction from "./DevelopmentReloadAction";

afterEach(cleanup);

test("explains a failed build without implying that workers or the current app stopped", () => {
  const reload = vi.fn().mockResolvedValue(undefined);
  render(<DevelopmentReloadAction busy={false} onReload={reload} runtime={{
    enabled: true,
    version: "0.1.0-dev-123456789abc-20260815040000-10",
    state: "failed",
    reload_available: true,
    deployed_source_revision: "76543210fedc",
    source_revision: "abcdef012345",
    source_dirty: false,
    deployed_source_published: true,
  }} />);

  const status = screen.getByLabelText("App and API status");
  expect(status).toHaveTextContent("Development build failed");
  expect(status).toHaveTextContent("Revision 7654321 is still serving this page");
  expect(status).toHaveTextContent("Workers were never restarted or interrupted");
  fireEvent.click(screen.getByRole("button", { name: "Retry development build" }));
  fireEvent.click(screen.getByRole("button", { name: "Build and reload" }));
  expect(reload).toHaveBeenCalledOnce();
});

test("names both revisions while a safe app reload is available", () => {
  render(<DevelopmentReloadAction busy={false} onReload={vi.fn()} runtime={{
    enabled: true,
    version: "0.1.0-dev-123456789abc-20260815040000-10",
    state: "idle",
    reload_available: true,
    deployed_source_revision: "76543210fedc",
    source_revision: "abcdef012345",
    source_dirty: false,
    deployed_source_published: true,
  }} />);

  expect(screen.getByLabelText("App and API status")).toHaveTextContent(
    "Revision 7654321 is active. Build and switch the browser and API to working-copy revision abcdef0.",
  );
});

test("explains that the running build matches the polled working copy", () => {
  render(<DevelopmentReloadAction busy={false} onReload={vi.fn()} runtime={{
    enabled: true,
    version: "0.1.0-dev-123456789abc-20260815040000-10",
    state: "ready",
    reload_available: false,
    deployed_source_revision: "abcdef012345",
    source_revision: "abcdef012345",
    source_dirty: false,
    deployed_source_published: true,
  }} />);

  const status = screen.getByLabelText("App and API status");
  expect(status).toHaveTextContent("Running build matches the working copy");
  expect(status).toHaveTextContent("Active revision abcdef0 matches the product code in this checkout");
  expect(status).toHaveTextContent("Swarm checks the working copy every 15 seconds");
  expect(status).toHaveTextContent("without restarting Claude, Codex, or the worker engine");
});

test("blocks an older or unrelated development checkout", () => {
  render(<DevelopmentReloadAction busy={false} onReload={vi.fn()} runtime={{
    enabled: true,
    version: "0.1.0-123456789abc",
    state: "source_mismatch",
    reload_available: false,
    deployed_source_revision: "76543210fedc",
    source_revision: "abcdef012345",
    source_dirty: false,
    deployed_source_published: true,
  }} />);

  const status = screen.getByLabelText("App and API status");
  expect(status).toHaveTextContent("Development checkout needs to catch up");
  expect(status).toHaveTextContent("Revision 7654321 is active");
  expect(status).toHaveTextContent("working-copy revision abcdef0 does not contain that deployed source");
  expect(screen.queryByRole("button", { name: /reload|build/i })).not.toBeInTheDocument();
});

test("shows a build in progress the way the worker engine card does", () => {
  // The build ran with nothing but a change of wording to show for it, so the
  // operator could not find it. It now carries the same live progress block as
  // the worker engine update, in the card where they started it.
  const { container } = render(<DevelopmentReloadAction busy={false} onReload={vi.fn()} runtime={{
    enabled: true,
    version: "0.1.0-dev-123456789abc-20260815040000-10",
    state: "building",
    reload_available: false,
    deployed_source_revision: "76543210fedc",
    source_revision: "abcdef012345",
    source_dirty: false,
    deployed_source_published: true,
  }} />);

  const status = screen.getByLabelText("App and API status");
  expect(status).toHaveTextContent("Building App and API…");
  expect(status).toHaveTextContent("Compiling and checking abcdef0");
  expect(status).toHaveTextContent("Revision 7654321 keeps serving this page");
  expect(container.querySelector(".maintenance-spinner")).not.toBeNull();
});

test("separates a build that has been asked for from one that is running", () => {
  render(<DevelopmentReloadAction busy={false} onReload={vi.fn()} runtime={{
    enabled: true,
    version: "0.1.0-dev-123456789abc-20260815040000-10",
    state: "requested",
    reload_available: false,
    deployed_source_revision: "76543210fedc",
    source_revision: "abcdef012345",
    source_dirty: false,
    deployed_source_published: true,
  }} />);

  expect(screen.getByLabelText("App and API status")).toHaveTextContent("Starting the App and API build…");
});

test("holds its place while the API restarts under the build", () => {
  // The operator watched a build compile and then the whole App and API card
  // disappeared, with a 502 alongside it. Vanishing mid-operation reads as the
  // build having destroyed something.
  render(<DevelopmentReloadAction busy={false} reachable={false} onReload={vi.fn()} runtime={{
    enabled: true,
    version: "0.1.0-dev-123456789abc-20260815040000-10",
    state: "building",
    reload_available: false,
    deployed_source_revision: "76543210fedc",
    source_revision: "abcdef012345",
    source_dirty: false,
    deployed_source_published: true,
  }} />);

  const status = screen.getByLabelText("App and API status");
  expect(status).toHaveTextContent("Reconnecting…");
  expect(status).toHaveTextContent("expected while a new build takes over");
});

test("confirms the last build landed, even while offering the next one", () => {
  // The operator ran a build, watched it compile, and then saw the card
  // offering a reload again. It was right to — newer commits existed — but
  // nothing said whether the build they had asked for succeeded.
  render(<DevelopmentReloadAction busy={false} onReload={vi.fn()} runtime={{
    enabled: true,
    version: "0.1.0-dev-123456789abc-20260815040000-10",
    state: "ready",
    reload_available: true,
    deployed_source_revision: "a50fcb465413",
    source_revision: "9668d65abcde",
    source_dirty: false,
    deployed_source_published: true,
  }} />);

  const status = screen.getByLabelText("App and API status");
  expect(status).toHaveTextContent("The last build completed, and revision a50fcb4 is serving this page");
  expect(status).toHaveTextContent("Build and switch the browser and API to working-copy revision 9668d65");
});

/**
 * The running code exists only on this machine, and nothing said so.
 *
 * A reload builds from the local checkout — right for a tool whose developer is
 * its operator, and it makes the develop-and-reload loop work. What was
 * invisible is the consequence: this Hive ran a commit that was on no remote
 * for about twenty minutes. Both a worker and Queen reported that work as
 * "pushed, not deployed" when it was deployed and not pushed. Committed, pushed
 * and deployed are three claims, and the surface carried only the third.
 */
test("says when the running revision exists only on this machine", () => {
  render(<DevelopmentReloadAction busy={false} onReload={vi.fn()} runtime={{
    enabled: true,
    version: "0.1.0-dev-123456789abc-20260815040000-10",
    state: "ready",
    reload_available: false,
    deployed_source_revision: "a50fcb465413",
    source_revision: "a50fcb465413",
    source_dirty: false,
    deployed_source_published: false,
  }} />);

  const status = screen.getByLabelText("App and API status");
  expect(status).toHaveTextContent("is not on any remote");
  expect(status).toHaveTextContent("only on this machine");
});

test("says nothing about remotes once the running revision is pushed", () => {
  // Reported, never gated: the warning clears on its own when the commit lands
  // on a remote, without anyone dismissing anything.
  render(<DevelopmentReloadAction busy={false} onReload={vi.fn()} runtime={{
    enabled: true,
    version: "0.1.0-dev-123456789abc-20260815040000-10",
    state: "ready",
    reload_available: false,
    deployed_source_revision: "a50fcb465413",
    source_revision: "a50fcb465413",
    source_dirty: false,
    deployed_source_published: true,
  }} />);

  expect(screen.getByLabelText("App and API status")).not.toHaveTextContent("only on this machine");
});

test("says nothing about a last build when none has run", () => {
  render(<DevelopmentReloadAction busy={false} onReload={vi.fn()} runtime={{
    enabled: true,
    version: "0.1.0-dev-123456789abc-20260815040000-10",
    state: "idle",
    reload_available: true,
    deployed_source_revision: "a50fcb465413",
    source_revision: "9668d65abcde",
    source_dirty: false,
    deployed_source_published: true,
  }} />);

  expect(screen.getByLabelText("App and API status")).not.toHaveTextContent("The last build completed");
});

test("says uncommitted changes are the reason, not a revision that reads identical", () => {
  // Observed: both halves named ed715fe and it still offered a reload. The
  // working copy differed by uncommitted changes, which no revision comparison
  // can show, so the card named the same commit twice and explained nothing.
  render(<DevelopmentReloadAction busy={false} onReload={vi.fn()} runtime={{
    enabled: true,
    version: "0.1.0-dev-ed715fe3c3f3-20260820003934-235175",
    state: "ready",
    reload_available: true,
    deployed_source_revision: "ed715fe3c3f3",
    source_revision: "ed715fe3c3f3",
    source_dirty: true,
    deployed_source_published: true,
  }} />);

  const status = screen.getByLabelText("App and API status");
  expect(status).toHaveTextContent("the working copy has uncommitted changes on top of it");
  expect(status).not.toHaveTextContent("switch the browser and API to working-copy revision");
});

/**
 * The second meaning of "it is live", said at the moment it matters.
 *
 * A reload deliberately does not restart the terminal host — that is what keeps
 * worker terminals alive mid-turn — so it can report success while leaving the
 * engine behind. The operator met that as `Runtime request returned 422:
 * unknown variant "start_shell"`, which reads as a protocol bug rather than as
 * a service that never restarted.
 */
test("says the worker engine is behind even when the app matches the checkout", () => {
  // The case nothing else would mention: app and checkout agree, so this card
  // otherwise reads "Current" and there is no reload to hang the warning on.
  render(<DevelopmentReloadAction busy={false} onReload={vi.fn()} runtime={{
    enabled: true,
    version: "0.1.0-dev-123456789abc-20260815040000-10",
    state: "idle",
    reload_available: false,
    deployed_source_revision: "76543210fedc",
    source_revision: "76543210fedc",
    source_dirty: false,
    deployed_source_published: true,
    worker_engine_update_required: true,
  }} />);

  expect(screen.getByText(/The worker engine is behind this build/)).toBeInTheDocument();
  expect(screen.getByText(/a background check will install it without asking/)).toBeInTheDocument();
});

test("says the worker engine is behind while a reload is also available", () => {
  render(<DevelopmentReloadAction busy={false} onReload={vi.fn()} runtime={{
    enabled: true,
    version: "0.1.0-dev-123456789abc-20260815040000-10",
    state: "idle",
    reload_available: true,
    deployed_source_revision: "76543210fedc",
    source_revision: "abcdef012345",
    source_dirty: false,
    deployed_source_published: true,
    worker_engine_update_required: true,
  }} />);

  expect(screen.getByText(/The worker engine is behind this build/)).toBeInTheDocument();
});

test("says nothing about the engine when it is current", () => {
  render(<DevelopmentReloadAction busy={false} onReload={vi.fn()} runtime={{
    enabled: true,
    version: "0.1.0-dev-123456789abc-20260815040000-10",
    state: "idle",
    reload_available: false,
    deployed_source_revision: "76543210fedc",
    source_revision: "76543210fedc",
    source_dirty: false,
    deployed_source_published: true,
    worker_engine_update_required: false,
  }} />);

  expect(screen.queryByText(/The worker engine is behind/)).not.toBeInTheDocument();
});

test("says nothing about the engine when the host could not be asked", () => {
  // null is not false. A host that did not answer is not a host that is up to
  // date, and claiming current because nothing replied is the check-that-
  // cannot-fail shape this whole task exists to remove.
  render(<DevelopmentReloadAction busy={false} onReload={vi.fn()} runtime={{
    enabled: true,
    version: "0.1.0-dev-123456789abc-20260815040000-10",
    state: "idle",
    reload_available: false,
    deployed_source_revision: "76543210fedc",
    source_revision: "76543210fedc",
    source_dirty: false,
    deployed_source_published: true,
    worker_engine_update_required: null,
  }} />);

  expect(screen.queryByText(/The worker engine is behind/)).not.toBeInTheDocument();
});

/**
 * The operator: "if you have an update that needs the engine to restart, I
 * should see that notification."
 *
 * They were right and there was a hole. worker_engine_update_required compares
 * the RUNNING host against the RUNNING API — both are the build in service, so
 * a checkout that bumps PROTOCOL_VERSION leaves them agreeing and reports
 * nothing. The only way to find out was to press reload and be refused.
 */
test("a checkout that changes the protocol says so before the button is pressed", () => {
  render(<DevelopmentReloadAction busy={false} onReload={vi.fn()} runtime={{
    enabled: true,
    version: "0.1.0-dev-123456789abc-20260815040000-10",
    state: "idle",
    reload_available: true,
    deployed_source_revision: "76543210fedc",
    source_revision: "abcdef012345",
    source_dirty: false,
    deployed_source_published: true,
    worker_engine_update_required: false,
    protocol_migration_required: true,
  }} />);

  expect(screen.getByText(/This checkout changes the terminal-host protocol/)).toBeInTheDocument();
  expect(screen.getByText(/stops every worker/)).toBeInTheDocument();
});

test("it says so even when the app itself matches the checkout", () => {
  // The case nothing else would mention: the card otherwise reads Current and
  // there is no reload in flight to hang the warning on.
  render(<DevelopmentReloadAction busy={false} onReload={vi.fn()} runtime={{
    enabled: true,
    version: "0.1.0-dev-123456789abc-20260815040000-10",
    state: "idle",
    reload_available: false,
    deployed_source_revision: "76543210fedc",
    source_revision: "76543210fedc",
    source_dirty: false,
    deployed_source_published: true,
    protocol_migration_required: true,
  }} />);

  expect(screen.getByText(/This checkout changes the terminal-host protocol/)).toBeInTheDocument();
});

test("a checkout on the same protocol says nothing about migrating", () => {
  render(<DevelopmentReloadAction busy={false} onReload={vi.fn()} runtime={{
    enabled: true,
    version: "0.1.0-dev-123456789abc-20260815040000-10",
    state: "idle",
    reload_available: true,
    deployed_source_revision: "76543210fedc",
    source_revision: "abcdef012345",
    source_dirty: false,
    deployed_source_published: true,
    protocol_migration_required: false,
  }} />);

  expect(screen.queryByText(/changes the terminal-host protocol/)).not.toBeInTheDocument();
});

/**
 * ⚠️ THE SENTENCE THAT COST NINETY MINUTES SAID EVERY TRUE THING AND STILL MISLED.
 *
 * "A reload does not restart it — that is what keeps worker terminals alive — so
 * run the worker engine update when your workers are idle." Each clause correct.
 * Together they invite the operator to do something later that a background timer
 * does for them, on exactly the trigger the sentence names: idleness.
 *
 * On 2026-09-03 the timer deferred three times while workers were mid-turn and
 * swapped at the first moment all ten were resting, stopping every session. The
 * operator WAS told the engine was behind. Nothing told them a timer would act on
 * it, or that the workers would not come back.
 *
 * These guard the three things the sentence must now carry, because an edit for
 * brevity would drop exactly those.
 */
test("the engine-behind notice says the swap happens without being asked", () => {
  render(<DevelopmentReloadAction busy={false} onReload={vi.fn()} runtime={{
    enabled: true,
    version: "0.1.0-dev-123456789abc-20260815040000-10",
    state: "idle",
    reload_available: false,
    deployed_source_revision: "76543210fedc",
    source_revision: "76543210fedc",
    source_dirty: false,
    deployed_source_published: true,
    worker_engine_update_required: true,
    running_worker_sessions: 11,
  }} />);
  expect(screen.getByText(/install it without asking/)).toBeInTheDocument();
  expect(screen.getByText(/first moment no worker is mid-turn/)).toBeInTheDocument();
});

test("it names how many sessions the swap will stop", () => {
  render(<DevelopmentReloadAction busy={false} onReload={vi.fn()} runtime={{
    enabled: true,
    version: "0.1.0-dev-123456789abc-20260815040000-10",
    state: "idle",
    reload_available: false,
    deployed_source_revision: "76543210fedc",
    source_revision: "76543210fedc",
    source_dirty: false,
    deployed_source_published: true,
    worker_engine_update_required: true,
    running_worker_sessions: 11,
  }} />);
  expect(screen.getByText(/all 11 running worker sessions/)).toBeInTheDocument();
});

test("it distinguishes recorded worker recovery from confirmed context restoration", () => {
  render(<DevelopmentReloadAction busy={false} onReload={vi.fn()} runtime={{
    enabled: true,
    version: "0.1.0-dev-123456789abc-20260815040000-10",
    state: "idle",
    reload_available: false,
    deployed_source_revision: "76543210fedc",
    source_revision: "76543210fedc",
    source_dirty: false,
    deployed_source_published: true,
    worker_engine_update_required: true,
    running_worker_sessions: 11,
  }} />);
  expect(screen.getByText(/records loaded workers for recovery before the swap/)).toBeInTheDocument();
  expect(screen.getByText(/not confirmation that context was restored/)).toBeInTheDocument();
});

/**
 * A count the host could not supply must not render "all null sessions" and must
 * not quietly read as zero. Not knowing and knowing none are different facts —
 * the rule the staleness flag beside it already follows.
 */
test("an unknown session count degrades to a sentence rather than a number", () => {
  render(<DevelopmentReloadAction busy={false} onReload={vi.fn()} runtime={{
    enabled: true,
    version: "0.1.0-dev-123456789abc-20260815040000-10",
    state: "idle",
    reload_available: false,
    deployed_source_revision: "76543210fedc",
    source_revision: "76543210fedc",
    source_dirty: false,
    deployed_source_published: true,
    worker_engine_update_required: true,
  }} />);
  expect(screen.getByText(/every running worker session/)).toBeInTheDocument();
  expect(screen.queryByText(/all null/)).not.toBeInTheDocument();
});

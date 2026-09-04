import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import type { ComponentProps } from "react";
import { afterEach, expect, test, vi } from "vitest";

import SettingsWorkspace from "./SettingsWorkspace";
import { jiraStatusLabel, resourceLabel } from "./DiagnosticsWorkspace";

const originalScrollIntoView = HTMLElement.prototype.scrollIntoView;

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  if (originalScrollIntoView) HTMLElement.prototype.scrollIntoView = originalScrollIntoView;
  else delete (HTMLElement.prototype as Partial<HTMLElement>).scrollIntoView;
});

// This one test drives ten unrelated things — navigation, presence policy, lock
// detection, Queen policy, resources, saved reports, and the theme — across a
// hundred and sixty lines and seven separate waits. Each wait is a chance to
// exceed the default one-second budget when the whole suite is competing for
// CPU, which is why it failed under load and passed alone. The waits are given
// room here because they assert behaviour, not speed; the size is the reason
// there are so many of them to trip over.
test("shows subsystem diagnostics, previews a sanitized report, and changes the selected theme", async () => {
  const createObjectURL = vi.fn().mockReturnValue("blob:dogfood-screenshot");
  const revokeObjectURL = vi.fn();
  vi.stubGlobal("URL", { ...URL, createObjectURL, revokeObjectURL });
  const click = vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => undefined);
  const onThemeChange = vi.fn();
  const onPresenceChange = vi.fn().mockResolvedValue(undefined);
  const onEnableLockDetection = vi.fn().mockResolvedValue(undefined);
  const onNotificationPolicyChange = vi.fn().mockResolvedValue(undefined);
  const onQueenPolicyChange = vi.fn().mockResolvedValue(undefined);
  const onEnableNotifications = vi.fn().mockResolvedValue(undefined);
  const onDisableNotifications = vi.fn().mockResolvedValue(undefined);
  const onTestNotification = vi.fn().mockResolvedValue(undefined);
  const onUpdateWorkerEngine = vi.fn().mockResolvedValue(undefined);
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL) => {
    const url = String(input);
    if (url.includes("feedback/attachments/screen-123.png")) {
      return new Response(new Uint8Array([137, 80, 78, 71]), { status: 200, headers: { "Content-Type": "image/png" } });
    }
    if (url.includes("integrations/jira/readiness")) return ok({ configured: false, connection: "not_connected", account_name: null });
    if (url.includes("integrations/jira/bindings")) return ok([]);
    if (url.includes("feedback/reports")) return ok([{
      id: "report-1",
      expectation: "Worker remains readable",
      observation: "Terminal wrapped too narrowly",
      diagnostic_bundle: "sanitized saved evidence",
      attachment_name: "screen-123.png",
      created_at: 1_786_000_000,
    }]);
    if (url.includes("terminal-host")) return ok({ type: "host_status", status: { protocol_version: 5, host_version: "0.1.0-host", draining: false, running_sessions: 1, retained_sessions: 3 } });
    if (url.includes("runtime/development")) return ok({ enabled: true, version: "0.1.0", state: "idle", reload_available: false, source_revision: "current", source_dirty: false });
    if (url.includes("runtime/resources")) return ok({
      sampled_at: 1,
      policy: {
        mode: "observe_only",
        advisory_bytes: 268_435_456,
        critical_bytes: 536_870_912,
      },
      api: { resident_memory_bytes: 18_874_368, pressure: "normal" },
      terminal_host: {
        resident_memory_bytes: 9_437_184,
        process_tree_resident_memory_bytes: 508_559_360,
        process_tree_process_count: 8,
        // The server judges this against the machine. The fixture used to say
        // "critical" while the assertion below expected Normal, because the
        // page carried its own byte thresholds and ignored this field.
        pressure: "normal",
      },
    });
    return ok({ type: "history_diagnostics", diagnostics: { retained_bytes: 42, session_count: 3, segment_count: 1, dropped_records: 0, dropped_bytes: 0, recovered_truncated_bytes: 0, recovered_corrupt_segments: 0 } });
  }));
  const props: ComponentProps<typeof SettingsWorkspace> = {
    section: "settings-hive",
    busy: false,
    colorTheme: "light",
    onChooseWorkerMark: vi.fn(),
    startSurface: "tasks",
    onStartSurfaceChange: vi.fn(),
    onLock: vi.fn(),
    feedbackRevision: 0,
    liveFeedState: "connected",
    operatorToken: "secret-token",
    presence: { mode: "away", manual_mode: null, source: "screen_locked" },
    lockDetectionState: "available",
    notificationSettings: { policy: "important_only", subscription_count: 0, vapid_public_key: "public-key" },
    queenPolicy: { at_hive: "coordinate", away: "coordinate", night_watch: "local_execution" },
    providers: { claude_code: true, codex: false },
    notificationState: "available",
    recentEvents: [{ sequence: 7, hive_id: "hive-1", kind: "workers_changed", occurred_at: 1 }],
    hiveIdentity: { operator: { id: "operator-1", display_name: "Bea" }, hive: { id: "hive-1", name: "Meadow Hive", operator_id: "operator-1", apiary_id: null } },
    health: { status: "ok", version: "0.1.0" },
    sessions: [{ session_id: "session-safe-id", running: true }, { session_id: "session-2", running: false }, { session_id: "session-3", running: false }],
    workers: [{ id: "worker-1", hive_id: "hive-1", name: "Private name", role: "worker", provider: "claude_code", workspace: "/private/workspace", autostart: false, position: 1, active_session_id: "session-safe-id", created_at: 1, updated_at: 1, running: true, attention_state: "blocked", runtime_error: "raw provider failure detail" }],
    workspaces: [],
    onThemeChange: onThemeChange,
    onPresenceChange: onPresenceChange,
    onEnableLockDetection: onEnableLockDetection,
    onNotificationPolicyChange: onNotificationPolicyChange,
    onQueenPolicyChange: onQueenPolicyChange,
    onEnableNotifications: onEnableNotifications,
    onDisableNotifications: onDisableNotifications,
    onTestNotification: onTestNotification,
    onCreateWorker: vi.fn().mockResolvedValue(undefined),
    onUpdateWorker: vi.fn().mockResolvedValue(undefined),
    onRemoveWorker: vi.fn().mockResolvedValue(undefined),
    onReorderWorkers: vi.fn().mockResolvedValue(undefined),
    onRestartProviders: vi.fn(),
    onUpdateWorkerEngine: onUpdateWorkerEngine,
    onForceWorkerReload: vi.fn().mockResolvedValue(undefined),
    onReloadDevelopment: vi.fn().mockResolvedValue(undefined),
    onHiveIdentityChange: vi.fn(),
  };
  const { rerender } = render(<SettingsWorkspace {...props} section="settings-hive" />);

  // Section navigation lives in the rail now, with every other surface's, and
  // is covered at that level. This file covers what the workspace itself does.

  expect(screen.getByRole("status", { name: "Current presence" })).toHaveTextContent("ReachableComputer lock detected");
  fireEvent.change(screen.getByLabelText("Presence policy"), { target: { value: "night_watch" } });
  expect(onPresenceChange).toHaveBeenCalledWith("night_watch");
  fireEvent.click(screen.getByRole("button", { name: "Enable" }));
  expect(onEnableLockDetection).toHaveBeenCalledOnce();
  expect(screen.getByText("Available when you choose")).toBeInTheDocument();
  // Scoped to the shortcut list: the opening-screen chooser names the same
  // surfaces, so an unscoped query now matches both.
  const shortcuts = document.querySelector(".shortcut-list") as HTMLElement;
  expect(within(shortcuts).getByText("Needs you").parentElement).toHaveTextContent("Needs youAlt1");
  expect(within(shortcuts).getByText("Tasks").parentElement).toHaveTextContent("TasksAlt2");
  expect(within(shortcuts).getByText("Workers").parentElement).toHaveTextContent("WorkersAlt3");
  expect(within(shortcuts).getByText("Settings").parentElement).toHaveTextContent("SettingsAlt4");
  expect(screen.getByText("Quick navigation").parentElement).toHaveTextContent("Quick navigationAltK");

  fireEvent.change(screen.getByLabelText("Notify me"), { target: { value: "all_decisions" } });
  expect(onNotificationPolicyChange).toHaveBeenCalledWith("all_decisions");
  fireEvent.click(screen.getByRole("button", { name: "Enable this device" }));
  expect(onEnableNotifications).toHaveBeenCalledOnce();

  // Queen's autonomy ceiling sits with the workers it governs.
  rerender(<SettingsWorkspace {...props} section="settings-workers" onQueenPolicyChange={onQueenPolicyChange} />);
  fireEvent.change(screen.getByLabelText("Reachable"), { target: { value: "advisory" } });
  expect(onQueenPolicyChange).toHaveBeenCalledWith({ at_hive: "coordinate", away: "advisory", night_watch: "local_execution" });

  // Who this Hive is, and what it is connected to.
  rerender(<SettingsWorkspace {...props} section="settings-connections" />);
  expect(screen.getAllByText("Meadow Hive").length).toBeGreaterThan(0);
  expect(screen.getByText("Bea")).toBeInTheDocument();
  expect(screen.getAllByText("Personal Hive").length).toBeGreaterThan(0);
  expect(await screen.findByText("Jira not connected", {}, { timeout: 5_000 })).toBeInTheDocument();
  expect(screen.getByRole("heading", { name: "Bring Jira into your Hive" }).closest("section")).toHaveTextContent("Owned tasks continue; new shared claims wait.");

  // What is running, and what an update would cost.
  rerender(<SettingsWorkspace {...props} section="settings-updates" onUpdateWorkerEngine={onUpdateWorkerEngine} />);
  expect(screen.getByText("Live updates").parentElement).toHaveTextContent("Live updatesConnected");
  expect(screen.getByText("Running workers").parentElement).toHaveTextContent("Running workers1");
  expect(screen.getByText("Retained sessions").parentElement).toHaveTextContent("Retained sessions3");
  expect(screen.getByLabelText("Worker engine status")).toHaveTextContent("Worker engineUpdate ready · restart requiredRestart required");
  expect(screen.getByLabelText("Worker engine status")).toHaveTextContent("briefly stops 1 active worker");
  // This worker is blocked, not mid-command, so the update costs nothing in
  // progress and the card says so rather than warning generically.
  expect(screen.getByLabelText("Worker engine status")).toHaveTextContent("nothing in progress is lost");
  expect(screen.getByLabelText("App and API status")).toHaveTextContent("App and APIRunning build matches the working copyCurrent");
  expect(screen.getByLabelText("App and API status")).toHaveTextContent("checks the working copy every 15 seconds");
  fireEvent.click(screen.getByRole("button", { name: "Prepare worker engine update" }));
  expect(screen.getByRole("group", { name: "Confirm worker engine update" })).toHaveTextContent("Restart 1 active worker now?");
  fireEvent.click(screen.getByRole("button", { name: "Stop workers and update" }));
  expect(onUpdateWorkerEngine).toHaveBeenCalledOnce();

  // Everything the operator would send to support.
  rerender(<SettingsWorkspace {...props} section="settings-maintenance" />);
  // Healthy checks collapse now: the page leads with its verdict and keeps the
  // evidence behind a count. These rows are all healthy in this fixture, so the
  // full list has to be asked for.
  fireEvent.click(await screen.findByRole("button", { name: /Show all \d+ checks/ }, { timeout: 5_000 }));
  const terminalHost = await screen.findByText("Terminal host", {}, { timeout: 5_000 });
  expect(terminalHost.parentElement).toHaveTextContent("Terminal hostHealthy · 0.1.0-host");
  expect((await screen.findByText("API memory", {}, { timeout: 5_000 })).parentElement).toHaveTextContent("API memoryNormal · 18.0 MiB");
  expect(screen.getByText("Terminal host service").parentElement).toHaveTextContent("Terminal host service9.0 MiB");
  expect(screen.getByText("Loaded worker runtimes").parentElement).toHaveTextContent("Loaded worker runtimesNormal · 476.0 MiB · 1 loaded");
  expect(screen.getByText("Live metrics")).toBeInTheDocument();
  expect(screen.getByText(/refreshes every 10 seconds/)).toBeInTheDocument();
  expect(screen.getByText("Jira").parentElement).toHaveTextContent("JiraNot connected");
  const resourceRequests = () => vi.mocked(fetch).mock.calls.filter(([input]) => String(input).includes("runtime/resources")).length;
  const resourceRequestsBeforeRefresh = resourceRequests();
  fireEvent.click(screen.getByRole("button", { name: "Refresh now" }));
  await vi.waitFor(() => expect(resourceRequests()).toBeGreaterThan(resourceRequestsBeforeRefresh), { timeout: 5_000 });
  const savedReportSummary = await screen.findByText("Terminal wrapped too narrowly", { selector: "summary span" }, { timeout: 5_000 });
  expect(savedReportSummary).toBeInTheDocument();
  fireEvent.click(savedReportSummary);
  expect(screen.getByText("Worker remains readable")).toBeInTheDocument();
  expect(screen.getByText("Attached privately")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Download screenshot" }));
  await vi.waitFor(() => expect(createObjectURL).toHaveBeenCalledOnce(), { timeout: 5_000 });
  expect(click).toHaveBeenCalledOnce();
  expect(revokeObjectURL).toHaveBeenCalledWith("blob:dogfood-screenshot");

  fireEvent.click(screen.getByRole("button", { name: "Preview report" }));
  const preview = screen.getByLabelText("Sanitized diagnostic report");
  expect(preview).toHaveTextContent("session-safe-id");
  expect(preview).toHaveTextContent("workers_changed");
  expect(preview).toHaveTextContent("observe_only");
  expect(preview).toHaveTextContent("18874368");
  expect(preview).not.toHaveTextContent("Private name");
  expect(preview).not.toHaveTextContent("/private/workspace");
  expect(preview).not.toHaveTextContent("secret-token");
  expect(preview).not.toHaveTextContent("raw provider failure detail");

  // Diagnostics and the theme live in different sections now, and Settings
  // renders one section at a time, so the theme is exercised where it lives.
  rerender(
    <SettingsWorkspace
      {...minimalProps()}
      section="settings-hive"
      colorTheme="light"
      onThemeChange={onThemeChange}
    />,
  );
  expect(screen.getByRole("button", { name: "Light meadow" })).toHaveAttribute("aria-pressed", "true");
  fireEvent.click(screen.getByRole("button", { name: "Night hive" }));
  expect(onThemeChange).toHaveBeenCalledWith("dark");
});

test("uses distinct readable labels for every resource pressure state", () => {
  const resource = (pressure: "normal" | "advisory" | "critical", bytes = 10 * 1024 * 1024) =>
    ({ pressure, resident_memory_bytes: bytes });
  expect(resourceLabel(resource("normal"))).toBe("Normal · 10.0 MiB");
  expect(resourceLabel(resource("advisory"))).toBe("Watch · 10.0 MiB");
  expect(resourceLabel(resource("critical"))).toBe("Critical · 10.0 MiB");
  expect(resourceLabel({ pressure: "unavailable", resident_memory_bytes: null })).toBe("Unavailable");
  expect(resourceLabel(undefined)).toBe("Unavailable");
});

test("uses actionable Jira diagnostic states", () => {
  expect(jiraStatusLabel({ configured: true, accepts_api_token: false, connection: "ready", account_name: "Bea" }, false)).toBe("Connected · Bea");
  expect(jiraStatusLabel({ configured: true, accepts_api_token: false, connection: "network_unavailable", account_name: null }, false)).toBe("Network unavailable");
  expect(jiraStatusLabel({ configured: true, accepts_api_token: false, connection: "credentials_invalid", account_name: null }, false)).toBe("Credentials need attention");
  expect(jiraStatusLabel(undefined, true)).toBe("Unavailable");
});

/**
 * Replaces two tests that asserted a linked section was scrolled into view.
 *
 * That behaviour existed because the page rendered all fourteen cards at once
 * and the selected one had to be found among them, chased with a frame and two
 * timers that re-ran whenever the layout crossed phone width. Settings renders
 * one section now, so the requirement is simply that choosing a section puts
 * you at the top of it.
 */
test("choosing a section puts the operator at the top of it", async () => {
  const scrollTo = vi.fn();
  Object.defineProperty(HTMLElement.prototype, "scrollTo", { configurable: true, value: scrollTo });
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL) => {
    const url = String(input);
    if (url.includes("integrations/jira/readiness")) return ok({ configured: false, connection: "not_connected", account_name: null });
    if (url.includes("integrations/jira/bindings") || url.includes("feedback/reports")) return ok([]);
    if (url.includes("integrations/email/readiness")) return ok({ configured: false, connection: "not_connected", account_name: null });
    if (url.includes("runtime/resources")) return ok({ sampled_at: 1, policy: { mode: "observe_only", advisory_bytes: 1, critical_bytes: 2 }, api: { resident_memory_bytes: 1, pressure: "normal" }, terminal_host: { resident_memory_bytes: 1, pressure: "normal" } });
    if (url.includes("terminal-host")) return ok({ type: "host_status", status: { protocol_version: 7, host_version: "0.1.0", draining: false, running_sessions: 0, retained_sessions: 0 } });
    if (url.includes("runtime/development")) return ok({ enabled: false, version: "0.1.0", state: "idle", reload_available: false, source_revision: null, source_dirty: false });
    return ok({ type: "history_diagnostics", diagnostics: null });
  }));

  const { rerender } = render(<SettingsWorkspace {...minimalProps()} section="settings-hive" />);
  scrollTo.mockClear();

  rerender(<SettingsWorkspace {...minimalProps()} section="settings-maintenance" />);

  await waitFor(() => expect(scrollTo).toHaveBeenCalledWith({ top: 0, behavior: "auto" }));
});

/**
 * The defect this whole regrouping came from: four cards were in the page and
 * in no section, so the operator went looking for a passkey and for the tunnel
 * and found neither.
 */
test("a section shows its own cards and nothing else", async () => {
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL) => {
    const url = String(input);
    if (url.includes("integrations/jira/readiness")) return ok({ configured: false, connection: "not_connected", account_name: null });
    if (url.includes("integrations/jira/bindings") || url.includes("feedback/reports")) return ok([]);
    if (url.includes("integrations/email/readiness")) return ok({ configured: false, connection: "not_connected", account_name: null });
    if (url.includes("runtime/resources")) return ok({ sampled_at: 1, policy: { mode: "observe_only", advisory_bytes: 1, critical_bytes: 2 }, api: { resident_memory_bytes: 1, pressure: "normal" }, terminal_host: { resident_memory_bytes: 1, pressure: "normal" } });
    if (url.includes("terminal-host")) return ok({ type: "host_status", status: { protocol_version: 7, host_version: "0.1.0", draining: false, running_sessions: 0, retained_sessions: 0 } });
    if (url.includes("runtime/development")) return ok({ enabled: false, version: "0.1.0", state: "idle", reload_available: false, source_revision: null, source_dirty: false });
    if (url.includes("runtime/tunnel")) return ok({ available: true, running: false, url: null, started_at: null, qr_svg: null });
    return ok({ type: "history_diagnostics", diagnostics: null });
  }));

  const { container } = render(<SettingsWorkspace {...minimalProps()} section="settings-access" />);

  // Both access cards, together, where someone looking for either would go.
  await waitFor(() => expect(container.querySelector("#settings-access")).not.toBeNull());
  expect(container.querySelector("#settings-remote")).not.toBeNull();
  // And nothing from any other section.
  expect(container.querySelector("#settings-crew")).toBeNull();
  expect(container.querySelector("#settings-backup")).toBeNull();
});

/** Typing a word finds the card holding it, wherever it lives. */
test("the filter reaches a card the selected section does not contain", async () => {
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL) => {
    const url = String(input);
    if (url.includes("integrations/jira/readiness")) return ok({ configured: false, connection: "not_connected", account_name: null });
    if (url.includes("integrations/jira/bindings") || url.includes("feedback/reports")) return ok([]);
    if (url.includes("integrations/email/readiness")) return ok({ configured: false, connection: "not_connected", account_name: null });
    if (url.includes("runtime/resources")) return ok({ sampled_at: 1, policy: { mode: "observe_only", advisory_bytes: 1, critical_bytes: 2 }, api: { resident_memory_bytes: 1, pressure: "normal" }, terminal_host: { resident_memory_bytes: 1, pressure: "normal" } });
    if (url.includes("terminal-host")) return ok({ type: "host_status", status: { protocol_version: 7, host_version: "0.1.0", draining: false, running_sessions: 0, retained_sessions: 0 } });
    if (url.includes("runtime/development")) return ok({ enabled: false, version: "0.1.0", state: "idle", reload_available: false, source_revision: null, source_dirty: false });
    if (url.includes("runtime/tunnel")) return ok({ available: true, running: false, url: null, started_at: null, qr_svg: null });
    return ok({ type: "history_diagnostics", diagnostics: null });
  }));

  const { container } = render(
    <SettingsWorkspace {...minimalProps()} section="settings-maintenance" query="passkey" />,
  );

  await waitFor(() => expect(container.querySelector("#settings-access")).not.toBeNull());
  expect(container.querySelector("#settings-backup")).toBeNull();
  expect(screen.getByText('1 setting matches "passkey".')).toBeInTheDocument();
});

test("downloads a consistent Hive database snapshot", async () => {
  const createObjectURL = vi.fn().mockReturnValue("blob:hive-backup");
  const revokeObjectURL = vi.fn();
  vi.stubGlobal("URL", { ...URL, createObjectURL, revokeObjectURL });
  const click = vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => undefined);
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL) => {
    const url = String(input);
    if (url.includes("integrations/jira/readiness")) return ok({ configured: false, connection: "not_connected", account_name: null });
    if (url.includes("integrations/jira/bindings")) return ok([]);
    if (url.includes("feedback/reports")) return ok([]);
    if (url === "/api/v1/backups/database") {
      return new Response(new Uint8Array([83, 81, 76]), { status: 200, headers: { "Content-Type": "application/vnd.sqlite3" } });
    }
    if (url.includes("runtime/resources")) return ok({
      sampled_at: 1, policy: { mode: "observe_only", advisory_bytes: 268_435_456, critical_bytes: 536_870_912 },
      api: { resident_memory_bytes: 1, pressure: "normal" }, terminal_host: { resident_memory_bytes: 1, pressure: "normal" },
    });
    if (url.includes("terminal-host")) return ok({ type: "host_status", status: { protocol_version: 5, host_version: "0.1.0", draining: false, running_sessions: 0, retained_sessions: 0 } });
    return ok({ type: "history_diagnostics", diagnostics: { retained_bytes: 0, session_count: 0, segment_count: 0, dropped_records: 0, dropped_bytes: 0, recovered_truncated_bytes: 0, recovered_corrupt_segments: 0 } });
  }));

  render(<SettingsWorkspace {...minimalProps()} section="settings-maintenance" />);
  fireEvent.click(screen.getByRole("button", { name: "Download Hive backup" }));

  await vi.waitFor(() => expect(createObjectURL).toHaveBeenCalledOnce());
  expect(click).toHaveBeenCalledOnce();
  expect(revokeObjectURL).toHaveBeenCalledWith("blob:hive-backup");
  expect(screen.getByText("Hive backup downloaded. Keep it somewhere private and durable.")).toBeInTheDocument();
  fireEvent.click(screen.getByText("How to restore this backup"));
  expect(screen.getByText(/swarm-next-package restore/)).toBeInTheDocument();
  expect(screen.getByText(/creates a rollback snapshot/)).toBeInTheDocument();
});

test("keeps backup failure visible and safely retryable", async () => {
  let backupAvailable = false;
  const createObjectURL = vi.fn().mockReturnValue("blob:hive-backup");
  vi.stubGlobal("URL", { ...URL, createObjectURL, revokeObjectURL: vi.fn() });
  vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => undefined);
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL) => {
    const url = String(input);
    if (url === "/api/v1/backups/database") {
      return backupAvailable
        ? new Response(new Uint8Array([83, 81, 76]), { status: 200, headers: { "Content-Type": "application/vnd.sqlite3" } })
        : new Response("backup unavailable", { status: 503 });
    }
    if (url.includes("integrations/jira/readiness")) return ok({ configured: false, connection: "not_connected", account_name: null });
    if (url.includes("integrations/email/readiness")) return ok({ configured: false, connection: "not_connected", account_name: null });
    if (url.includes("bindings") || url.includes("feedback/reports")) return ok([]);
    if (url.includes("terminal-host")) return ok({ type: "host_status", status: { protocol_version: 5, host_version: "0.1.0", draining: false, running_sessions: 0, retained_sessions: 0 } });
    if (url.includes("runtime/resources")) return ok({ sampled_at: 1, policy: { mode: "observe_only", advisory_bytes: 1, critical_bytes: 2 }, api: { resident_memory_bytes: 1, pressure: "normal" }, terminal_host: { resident_memory_bytes: 1, pressure: "normal" } });
    return ok({ type: "history_diagnostics", diagnostics: null });
  }));

  render(<SettingsWorkspace {...minimalProps()} section="settings-maintenance" />);
  fireEvent.click(screen.getByRole("button", { name: "Download Hive backup" }));
  expect(await screen.findByText("The Hive backup could not be prepared. No local data was changed.")).toBeInTheDocument();
  backupAvailable = true;
  fireEvent.click(screen.getByRole("button", { name: "Try backup again" }));
  await waitFor(() => expect(createObjectURL).toHaveBeenCalledOnce());
  expect(screen.queryByText("The Hive backup could not be prepared. No local data was changed.")).not.toBeInTheDocument();
});

test("confirms an opt-in development reload without implying worker loss", async () => {
  const onReloadDevelopment = vi.fn().mockResolvedValue(undefined);
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL) => {
    const url = String(input);
    if (url.includes("runtime/development")) return ok({ enabled: true, version: "0.1.0-dev", state: "idle", reload_available: true, source_revision: "123456789abc", source_dirty: false });
    if (url.includes("integrations/jira/readiness")) return ok({ configured: false, connection: "not_connected", account_name: null });
    if (url.includes("integrations/jira/bindings")) return ok([]);
    if (url.includes("feedback/reports")) return ok([]);
    if (url.includes("runtime/resources")) return ok({
      sampled_at: 1, policy: { mode: "observe_only", advisory_bytes: 268_435_456, critical_bytes: 536_870_912 },
      api: { resident_memory_bytes: 1, pressure: "normal" }, terminal_host: { resident_memory_bytes: 1, pressure: "normal" },
    });
    if (url.includes("terminal-host")) return ok({ type: "host_status", status: { protocol_version: 7, host_version: "0.1.0", draining: false, running_sessions: 2, retained_sessions: 2 } });
    return ok({ type: "history_diagnostics", diagnostics: { retained_bytes: 0, session_count: 0, segment_count: 0, dropped_records: 0, dropped_bytes: 0, recovered_truncated_bytes: 0, recovered_corrupt_segments: 0 } });
  }));

  render(<SettingsWorkspace {...minimalProps()} section="settings-updates" onReloadDevelopment={onReloadDevelopment} />);
  expect(await screen.findByRole("button", { name: "Reload development build" })).toBeInTheDocument();
  expect(screen.getByText("Development reload available", { selector: "strong" })).toBeInTheDocument();
  expect(screen.getByLabelText("App and API status")).toHaveTextContent("Workers stay online");
  fireEvent.click(screen.getByRole("button", { name: "Reload development build" }));
  expect(screen.getByRole("group", { name: "Confirm development reload" })).toHaveTextContent("Build and activate the working copy?");
  fireEvent.click(screen.getByRole("button", { name: "Build and reload" }));
  expect(onReloadDevelopment).toHaveBeenCalledOnce();
});

test("makes Queen automation observable, opt-in, and manually runnable", async () => {
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    if (url.endsWith("/orchestration/queen-automation/run")) {
      return ok(queenAutomation({ enabled: true, state: "running", actionable_count: 3, run_id: "run-safe" }));
    }
    if (url.endsWith("/orchestration/queen-automation")) {
      if (init?.method === "PUT") {
        const enabled = JSON.parse(String(init.body)).enabled as boolean;
        return ok(queenAutomation({ enabled }));
      }
      return ok(queenAutomation());
    }
    if (url.endsWith("/orchestration/coordinator")) {
      return ok({ completed_actions: 10, queen_calls_avoided: 7, uncertain_actions: 0, queued_actions: 1, stale_attention_actions: 2, worker_exit_attention_actions: 1, unstarted_attention_actions: 1, last_action_at: 100, automatic_start_admission: "deferred_advisory", automatic_start_batch_limit: 1 });
    }
    if (url.includes("integrations/jira/readiness")) return ok({ configured: false, connection: "not_connected", account_name: null });
    if (url.includes("integrations/jira/bindings") || url.includes("feedback/reports")) return ok([]);
    if (url.includes("integrations/email/readiness")) return ok({ configured: false, connection: "not_connected", account_name: null });
    if (url.includes("runtime/resources")) return ok({ sampled_at: 1, policy: { mode: "observe_only", advisory_bytes: 1, critical_bytes: 2 }, api: { resident_memory_bytes: 1, pressure: "normal" }, terminal_host: { resident_memory_bytes: 1, pressure: "normal" } });
    if (url.includes("terminal-host")) return ok({ type: "host_status", status: { protocol_version: 7, host_version: "0.1.0", draining: false, running_sessions: 0, retained_sessions: 0 } });
    if (url.includes("runtime/development")) return ok({ enabled: false, version: "0.1.0", state: "idle", reload_available: false, source_revision: null, source_dirty: false });
    return ok({ type: "history_diagnostics", diagnostics: null });
  }));

  render(<SettingsWorkspace {...minimalProps()} section="settings-workers" />);

  expect(await screen.findByText("Manual review only")).toBeInTheDocument();
  const toggle = screen.getByRole("checkbox", { name: "Automatic off" });
  expect(toggle).not.toBeChecked();
  expect(screen.getByText(/nothing runs automatically/)).toBeInTheDocument();
  expect(await screen.findByLabelText("Deterministic coordinator status")).toHaveTextContent("7Queen reviews avoided");
  expect(screen.getByLabelText("Deterministic coordinator status")).toHaveTextContent("1Worker starts queued");
  expect(screen.getByLabelText("Deterministic coordinator status")).toHaveTextContent("4" + "1 not started · 2 stale · 1 exited");
  expect(screen.getByLabelText("Deterministic coordinator status")).toHaveTextContent("waiting for memory pressure to settle");
  expect(screen.getByLabelText("Deterministic coordinator status")).toHaveTextContent("starts one worker at a time");
  expect(screen.getByLabelText("Deterministic coordinator status")).toHaveTextContent("delivered work that never starts");

  fireEvent.click(toggle);
  expect(await screen.findByRole("checkbox", { name: "Automatic on" })).toBeChecked();
  expect(vi.mocked(fetch).mock.calls.some(([input, init]) => String(input).endsWith("/orchestration/queen-automation") && init?.method === "PUT")).toBe(true);

  fireEvent.click(screen.getByRole("button", { name: "Run Queen now" }));
  expect(await screen.findByText("Queen is reviewing work")).toBeInTheDocument();
  expect(screen.getByText("3 actionable items in this review.")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Run Queen now" })).toBeDisabled();
});

test("routes an existing Queen decision to Needs you instead of repeating the review", async () => {
  const onOpenQueenDecisions = vi.fn();
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL) => {
    const url = String(input);
    if (url.endsWith("/orchestration/queen-automation")) {
      return ok(queenAutomation({ enabled: true, state: "uncertain", actionable_count: 8, run_id: "run-covered" }));
    }
    if (url.endsWith("/orchestration/coordinator")) {
      return ok({ completed_actions: 0, queen_calls_avoided: 0, uncertain_actions: 0, queued_actions: 0, stale_attention_actions: 0, worker_exit_attention_actions: 0, unstarted_attention_actions: 0, last_action_at: null, automatic_start_admission: "available", automatic_start_batch_limit: 1 });
    }
    if (url.includes("integrations/jira/readiness")) return ok({ configured: false, connection: "not_connected", account_name: null });
    if (url.includes("integrations/jira/bindings") || url.includes("feedback/reports")) return ok([]);
    if (url.includes("integrations/email/readiness")) return ok({ configured: false, connection: "not_connected", account_name: null });
    if (url.includes("runtime/resources")) return ok({ sampled_at: 1, policy: { mode: "observe_only", advisory_bytes: 1, critical_bytes: 2 }, api: { resident_memory_bytes: 1, pressure: "normal" }, terminal_host: { resident_memory_bytes: 1, pressure: "normal" } });
    if (url.includes("terminal-host")) return ok({ type: "host_status", status: { protocol_version: 7, host_version: "0.1.0", draining: false, running_sessions: 0, retained_sessions: 0 } });
    if (url.includes("runtime/development")) return ok({ enabled: false, version: "0.1.0", state: "idle", reload_available: false, source_revision: null, source_dirty: false });
    return ok({ type: "history_diagnostics", diagnostics: null });
  }));

  render(<SettingsWorkspace {...minimalProps()} section="settings-workers" pendingQueenDecisionCount={1} onOpenQueenDecisions={onOpenQueenDecisions} />);

  expect(await screen.findByText("Queen needs you")).toBeInTheDocument();
  expect(screen.getByText(/1 specific Queen decision is waiting in Needs you/)).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Retry Queen review" })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Review decision" }));
  expect(onOpenQueenDecisions).toHaveBeenCalledOnce();
  expect(vi.mocked(fetch).mock.calls.some(([input]) => String(input).endsWith("/orchestration/queen-automation/run"))).toBe(false);
});

test("does not label an unreachable worker engine as current", async () => {
  let hostAvailable = false;
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL) => {
    const url = String(input);
    if (url.includes("terminal-host")) {
      if (!hostAvailable) return new Response("host unavailable", { status: 502 });
      return ok({ type: "host_status", status: { protocol_version: 7, host_version: "0.1.0", draining: false, running_sessions: 1, retained_sessions: 1 } });
    }
    if (url.endsWith("/orchestration/queen-automation")) return ok(queenAutomation());
    if (url.endsWith("/orchestration/coordinator")) return ok({ completed_actions: 0, queen_calls_avoided: 0, uncertain_actions: 0, queued_actions: 0, stale_attention_actions: 0, worker_exit_attention_actions: 0, unstarted_attention_actions: 0, last_action_at: null, automatic_start_admission: "available", automatic_start_batch_limit: 1 });
    if (url.includes("integrations/jira/readiness")) return ok({ configured: false, connection: "not_connected", account_name: null });
    if (url.includes("integrations/email/readiness")) return ok({ configured: false, connection: "not_connected", account_name: null });
    if (url.includes("runtime/development")) return ok({ enabled: false, version: "0.1.0", state: "idle", reload_available: false, source_revision: null, source_dirty: false });
    if (url.includes("runtime/resources")) return ok({ sampled_at: 1, policy: { mode: "observe_only", advisory_bytes: 1, critical_bytes: 2 }, api: { resident_memory_bytes: 1, pressure: "normal" }, terminal_host: { resident_memory_bytes: null, pressure: "unavailable" } });
    if (url.includes("bindings") || url.includes("feedback/reports")) return ok([]);
    return ok({ type: "history_diagnostics", diagnostics: null });
  }));

  render(<SettingsWorkspace {...minimalProps()} section="settings-updates" />);

  const engine = await screen.findByLabelText("Worker engine status");
  expect(engine).toHaveTextContent("Unavailable");
  expect(engine).toHaveTextContent("No restart or update has been attempted.");
  expect(engine).not.toHaveTextContent("compatible with this App/API release");
  hostAvailable = true;
  fireEvent.click(screen.getByRole("button", { name: "Retry worker engine status" }));
  await waitFor(() => expect(engine).toHaveTextContent("Current · 1 active"));
});

test("keeps a confirmed worker engine steady when provider capabilities refresh", async () => {
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL) => {
    const url = String(input);
    if (url.includes("terminal-host")) return ok({ type: "host_status", status: { protocol_version: 7, host_version: "0.1.0", draining: false, running_sessions: 2, retained_sessions: 2 } });
    if (url.endsWith("/orchestration/queen-automation")) return ok(queenAutomation());
    if (url.endsWith("/orchestration/coordinator")) return ok({ completed_actions: 0, queen_calls_avoided: 0, uncertain_actions: 0, queued_actions: 0, stale_attention_actions: 0, worker_exit_attention_actions: 0, unstarted_attention_actions: 0, last_action_at: null, automatic_start_admission: "available", automatic_start_batch_limit: 1 });
    if (url.includes("integrations/jira/readiness") || url.includes("integrations/email/readiness")) return ok({ configured: false, connection: "not_connected", account_name: null });
    if (url.includes("runtime/development")) return ok({ enabled: false, version: "0.1.0", state: "idle", reload_available: false, source_revision: null, source_dirty: false });
    if (url.includes("runtime/resources")) return ok({ sampled_at: 1, policy: { mode: "observe_only", advisory_bytes: 1, critical_bytes: 2 }, api: { resident_memory_bytes: 1, pressure: "normal" }, terminal_host: { resident_memory_bytes: 1, pressure: "normal" } });
    if (url.includes("bindings") || url.includes("feedback/reports")) return ok([]);
    return ok({ type: "history_diagnostics", diagnostics: null });
  }));

  const view = render(<SettingsWorkspace {...minimalProps()} section="settings-updates" />);
  const engine = await screen.findByLabelText("Worker engine status");
  expect(engine).toHaveTextContent("Current · 2 active");
  const hostRequests = () => vi.mocked(fetch).mock.calls.filter(([input]) => String(input).includes("terminal-host")).length;
  const requestsBeforeProviderRefresh = hostRequests();

  view.rerender(<SettingsWorkspace {...minimalProps()} providers={{ claude_code: true, codex: true }} />);
  expect(engine).toHaveTextContent("Current · 2 active");
  expect(engine).not.toHaveTextContent("Checking");
  expect(hostRequests()).toBe(requestsBeforeProviderRefresh);
});

test("names the work a worker engine update would interrupt, before asking", async () => {
  // Raised as: this is the most harmful operation and has the least friction
  // around it. Loaded and working are different questions, and only the second
  // costs the operator anything.
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL) => {
    const url = String(input);
    // A host on a different build is what makes an update available at all.
    if (url.includes("terminal-host")) return ok({ type: "host_status", status: { protocol_version: 7, host_version: "0.1.0-old", draining: false, running_sessions: 2, retained_sessions: 2 } });
    if (url.endsWith("/orchestration/queen-automation")) return ok(queenAutomation());
    if (url.endsWith("/orchestration/coordinator")) return ok({ completed_actions: 0, queen_calls_avoided: 0, uncertain_actions: 0, queued_actions: 0, stale_attention_actions: 0, worker_exit_attention_actions: 0, unstarted_attention_actions: 0, last_action_at: null, automatic_start_admission: "available", automatic_start_batch_limit: 1 });
    if (url.includes("integrations/jira/readiness")) return ok({ configured: false, connection: "not_connected", account_name: null });
    if (url.includes("integrations/email/readiness")) return ok({ configured: false, connection: "not_connected", account_name: null });
    if (url.includes("runtime/development")) return ok({ enabled: false, version: "0.1.0", state: "idle", reload_available: false, source_revision: null, source_dirty: false });
    if (url.includes("runtime/resources")) return ok({ sampled_at: 1, policy: { mode: "observe_only", advisory_bytes: 1, critical_bytes: 2 }, api: { resident_memory_bytes: 1, pressure: "normal" }, terminal_host: { resident_memory_bytes: null, pressure: "unavailable" } });
    if (url.includes("bindings") || url.includes("feedback/reports")) return ok([]);
    return ok({ type: "history_diagnostics", diagnostics: null });
  }));
  render(<SettingsWorkspace {...minimalProps()} section="settings-updates"
    health={{ status: "ok", version: "0.1.0" }}
    workers={[
      { id: "w1", hive_id: "hive-1", name: "Queen", role: "queen", provider: "claude_code", workspace: "/w", autostart: true, position: 1, active_session_id: "s1", created_at: 1, updated_at: 1, running: true, attention_state: "buzzing" },
      { id: "w2", hive_id: "hive-1", name: "BudgetBug", role: "worker", provider: "claude_code", workspace: "/w", autostart: false, position: 2, active_session_id: "s2", created_at: 1, updated_at: 1, running: true, attention_state: "buzzing" },
      { id: "w3", hive_id: "hive-1", name: "Sculpt Studio", role: "worker", provider: "claude_code", workspace: "/w", autostart: false, position: 3, active_session_id: null, created_at: 1, updated_at: 1, running: false, attention_state: "resting" },
    ]}
  />);

  const engine = await screen.findByLabelText("Worker engine status");
  await waitFor(() => expect(engine).toHaveTextContent("2 workers are running a command right now: Queen, BudgetBug"));
  expect(engine).toHaveTextContent("not resumed");
  // And the same cost is restated where the operator commits to it, not only
  // where they first read about it.
  fireEvent.click(screen.getByRole("button", { name: "Prepare worker engine update" }));
  expect(screen.getByRole("group", { name: "Confirm worker engine update" }))
    .toHaveTextContent("2 workers are running a command right now");
});

function minimalProps() {
  return {
    // Tests that exercise a specific card pass the section that now holds it.
    section: "settings-hive" as const,
    busy: false, colorTheme: "light" as const, feedbackRevision: 0, liveFeedState: "connected" as const, operatorToken: "secret-token",
    presence: { mode: "at_hive" as const, manual_mode: null, source: "active_device" as const }, lockDetectionState: "unsupported" as const,
    notificationSettings: { policy: "important_only" as const, subscription_count: 0, vapid_public_key: "public-key" }, queenPolicy: { at_hive: "coordinate" as const, away: "coordinate" as const, night_watch: "local_execution" as const }, notificationState: "available" as const,
    recentEvents: [], sessions: [], workers: [], workspaces: [], providers: { claude_code: true, codex: false }, health: { status: "ok" as const, version: "0.1.0" },
    hiveIdentity: { operator: { id: "operator-1", display_name: "Bea" }, hive: { id: "hive-1", name: "Meadow Hive", operator_id: "operator-1", apiary_id: null } },
    onThemeChange: vi.fn(), onPresenceChange: vi.fn(), onEnableLockDetection: vi.fn(), onNotificationPolicyChange: vi.fn(),
    onQueenPolicyChange: vi.fn(), onEnableNotifications: vi.fn(), onDisableNotifications: vi.fn(), onTestNotification: vi.fn(), onCreateWorker: vi.fn(), onUpdateWorker: vi.fn(),
    onChooseWorkerMark: vi.fn(), onRemoveWorker: vi.fn(), onReorderWorkers: vi.fn(), onRestartProviders: vi.fn(), onUpdateWorkerEngine: vi.fn(), onForceWorkerReload: vi.fn(), onReloadDevelopment: vi.fn(), onHiveIdentityChange: vi.fn(),
  startSurface: "tasks",
  onStartSurfaceChange: vi.fn(),
  onLock: vi.fn(),
  };
}
function ok(body: unknown) {
  return new Response(JSON.stringify(body), { status: 200, headers: { "Content-Type": "application/json" } });
}

function queenAutomation(overrides: Record<string, unknown> = {}) {
  return {
    enabled: false,
    state: "idle",
    run_id: null,
    trigger: null,
    actionable_count: 0,
    attempts: 0,
    requested_at: null,
    delivered_at: null,
    finished_at: null,
    outcome: null,
    waiting_reason: null,
    ...overrides,
  };
}

test("chooses the screen Swarm opens on, for every device", () => {
  // "We should have an option in settings as to the start up screen, since
  // mobile always goes there." One choice, deliberately not per device class
  // like the theme beside it — a phone opening somewhere a desktop would not is
  // the thing this exists to stop.
  const onStartSurfaceChange = vi.fn();
  render(<SettingsWorkspace {...minimalProps()} startSurface="tasks" onStartSurfaceChange={onStartSurfaceChange} />);

  const choice = screen.getByRole("combobox", { name: /Opening screen/ });
  expect(choice).toHaveValue("tasks");
  expect(screen.getByText("Every device opens here.")).toBeInTheDocument();

  fireEvent.change(choice, { target: { value: "workers" } });
  expect(onStartSurfaceChange).toHaveBeenCalledWith("workers");
});

test("keeps locking reachable without giving it a place in every header", () => {
  // "The lock button seems a little bit silly that it's located on every single
  // page. It feels like a waste of real estate because how often are people
  // actually going to lock their terminal when they just lock their computer?"
  //
  // Locking Swarm while leaving the machine unlocked is a rare thing to want,
  // so it is reachable rather than resident — here, and in the command palette.
  const onLock = vi.fn();
  render(<SettingsWorkspace {...minimalProps()} onLock={onLock} />);

  fireEvent.click(screen.getByRole("button", { name: "Lock" }));

  expect(onLock).toHaveBeenCalledOnce();
});

/**
 * The engine card can be entirely correct and the workers still stale.
 *
 * A worker caches its MCP tool list at connect, so a changed agent tool surface
 * reaches nobody until the session reconnects — and nothing announces it. On
 * 2026-09-02 the API served tool surface revision 11 while all 13 live sessions
 * held revision 10, the engine card said Current (truthfully), and the operator
 * reasonably reported seeing no worker cycle pending. This control is the lever
 * for that, so it must be present when the engine is current.
 */
test("offers a forced worker reload even when the worker engine is current", async () => {
  const onForceWorkerReload = vi.fn().mockResolvedValue(undefined);
  render(<SettingsWorkspace {...minimalProps()} section="settings-updates" onForceWorkerReload={onForceWorkerReload} />);

  const open = await screen.findByRole("button", { name: "Force worker reload" });
  fireEvent.click(open);

  // It ends every live session, so it asks first rather than firing on a click.
  expect(onForceWorkerReload).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Restart every worker" }));
  expect(onForceWorkerReload).toHaveBeenCalledOnce();
});

/**
 * Two version numbers and a Current badge reads as "you are behind" when the
 * engine has not changed at all. host_version is the version of the release the
 * running host process was LAUNCHED from; the engine is identified by its build
 * id, and when those match there is nothing to apply.
 *
 * Measured 2026-09-02: Running 1.1.2 beside Installed 1.2.1, badge Current, no
 * upgrade offered — all correct, because both releases carried engine
 * dc6beb7c6604 and no file changed under the engine crates between them. The
 * operator asked three times why no upgrade was listed.
 */
test("explains a version gap the engine build id says is not a gap", async () => {
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL) => {
    const url = String(input);
    if (url.includes("terminal-host")) {
      return ok({ type: "host_status", status: {
        protocol_version: 10, host_version: "1.1.2", host_build_id: "dc6beb7c6604aaaa",
        draining: false, running_sessions: 13, retained_sessions: 13,
      } });
    }
    return ok({});
  }));

  render(<SettingsWorkspace
    {...minimalProps()}
    section="settings-updates"
    health={{ status: "ok", version: "1.2.1", worker_engine_build_id: "dc6beb7c6604aaaa" } as never}
  />);

  // The sentence is split across elements by the inline code span, so match on
  // the rendered text of the paragraph rather than a single text node.
  const explained = await screen.findByText(
    (_text, element) => element?.className === "engine-same-build",
  );
  expect(explained.textContent).toContain("dc6beb7c6604");
  expect(explained.textContent).toContain("nothing to upgrade");

  // ⚠️ AND THE VERDICT COMES FIRST. The explanation was correct and it was the
  // third paragraph; above it sat "Running 1.1.2" and "Installed 1.2.1" as
  // headings, which reads as being behind. The operator, looking at exactly
  // this state: "If a worker engine update isn't needed this should say so."
  expect(screen.getByText(/No worker engine update is needed/)).toBeInTheDocument();

  // The version that would be installed is NOT printed as a second heading when
  // there is nothing to install — it belongs in the explanation, which names it.
  expect(screen.queryByText("Installed")).not.toBeInTheDocument();
});

/**
 * The tool-surface control is not part of the worker engine and must say so.
 *
 * It sat unlabelled at the bottom of the engine card, distinguished only by a
 * sentence UNDER its own button. The operator: "The force worker reload section
 * should be clear that isn't related. the UI is confusing."
 */
test("separates the agent tool surface from the worker engine", async () => {
  vi.stubGlobal("fetch", vi.fn(async () => ok({})));

  render(<SettingsWorkspace {...minimalProps()} section="settings-updates" />);

  const heading = await screen.findByText("Agent tool surface");
  expect(heading).toBeInTheDocument();
  expect(heading.parentElement?.textContent).toContain("Not the worker engine");
  // The control it labels is still there and still reachable.
  expect(screen.getByRole("button", { name: "Force worker reload" })).toBeInTheDocument();
});

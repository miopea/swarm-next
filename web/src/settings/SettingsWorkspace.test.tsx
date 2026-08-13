import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import SettingsWorkspace from "./SettingsWorkspace";
import { resourceLabel } from "./DiagnosticsWorkspace";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

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
    if (url.includes("feedback/reports")) return ok([{
      id: "report-1",
      expectation: "Worker remains readable",
      observation: "Terminal wrapped too narrowly",
      diagnostic_bundle: "sanitized saved evidence",
      attachment_name: "screen-123.png",
      created_at: 1_786_000_000,
    }]);
    if (url.includes("terminal-host")) return ok({ type: "host_status", status: { protocol_version: 5, host_version: "0.1.0-host", draining: false, running_sessions: 1, retained_sessions: 3 } });
    if (url.includes("runtime/resources")) return ok({
      sampled_at: 1,
      policy: {
        mode: "observe_only",
        advisory_bytes: 268_435_456,
        critical_bytes: 536_870_912,
      },
      api: { resident_memory_bytes: 18_874_368, pressure: "normal" },
      terminal_host: { resident_memory_bytes: 9_437_184, pressure: "normal" },
    });
    return ok({ type: "history_diagnostics", diagnostics: { retained_bytes: 42, session_count: 3, segment_count: 1, dropped_records: 0, dropped_bytes: 0, recovered_truncated_bytes: 0, recovered_corrupt_segments: 0 } });
  }));
  render(
    <SettingsWorkspace
      busy={false}
      colorTheme="light"
      feedbackRevision={0}
      liveFeedState="connected"
      operatorToken="secret-token"
      presence={{ mode: "away", manual_mode: null, source: "screen_locked" }}
      lockDetectionState="available"
      notificationSettings={{ policy: "important_only", subscription_count: 0, vapid_public_key: "public-key" }}
      queenPolicy={{ at_hive: "coordinate", away: "coordinate", night_watch: "local_execution" }}
      providers={{ claude_code: true, codex: false }}
      notificationState="available"
      recentEvents={[{ sequence: 7, hive_id: "hive-1", kind: "workers_changed", occurred_at: 1 }]}
      hiveIdentity={{ operator: { id: "operator-1", display_name: "Bea" }, hive: { id: "hive-1", name: "Meadow Hive", operator_id: "operator-1", apiary_id: null } }}
      health={{ status: "ok", version: "0.1.0" }}
      sessions={[{ session_id: "session-safe-id", running: true }, { session_id: "session-2", running: false }, { session_id: "session-3", running: false }]}
      workers={[{ id: "worker-1", hive_id: "hive-1", name: "Private name", role: "worker", provider: "claude_code", workspace: "/private/workspace", autostart: false, position: 1, active_session_id: "session-safe-id", created_at: 1, updated_at: 1, running: true, attention_state: "blocked", runtime_error: "raw provider failure detail" }]}
      workspaces={[]}
      onThemeChange={onThemeChange}
      onPresenceChange={onPresenceChange}
      onEnableLockDetection={onEnableLockDetection}
      onNotificationPolicyChange={onNotificationPolicyChange}
      onQueenPolicyChange={onQueenPolicyChange}
      onEnableNotifications={onEnableNotifications}
      onDisableNotifications={onDisableNotifications}
      onTestNotification={onTestNotification}
      onCreateWorker={vi.fn().mockResolvedValue(undefined)}
      onUpdateWorker={vi.fn().mockResolvedValue(undefined)}
      onReorderWorkers={vi.fn().mockResolvedValue(undefined)}
      onUpdateWorkerEngine={onUpdateWorkerEngine}
    />,
  );

  const settingsNavigation = screen.getByRole("navigation", { name: "Settings sections" });
  expect(settingsNavigation).toHaveTextContent("CrewPresenceQueenAlertsSystemIntegrationsBackupDiagnostics");
  expect(screen.getByRole("button", { name: "Diagnostics" })).toHaveAttribute("aria-controls", "settings-diagnostics");
  fireEvent.click(screen.getByRole("button", { name: "Diagnostics" }));
  expect(screen.getByRole("button", { name: "Diagnostics" })).toHaveAttribute("aria-current", "location");
  expect(screen.getByRole("button", { name: "Crew" })).not.toHaveAttribute("aria-current");

  expect(screen.getAllByRole("status")[0]).toHaveTextContent("AwayComputer lock detected");
  fireEvent.change(screen.getByLabelText("Presence policy"), { target: { value: "night_watch" } });
  expect(onPresenceChange).toHaveBeenCalledWith("night_watch");
  fireEvent.click(screen.getByRole("button", { name: "Enable" }));
  expect(onEnableLockDetection).toHaveBeenCalledOnce();
  expect(screen.getByText("Available when you choose")).toBeInTheDocument();
  fireEvent.change(screen.getByLabelText("Away"), { target: { value: "advisory" } });
  expect(onQueenPolicyChange).toHaveBeenCalledWith({ at_hive: "coordinate", away: "advisory", night_watch: "local_execution" });
  fireEvent.change(screen.getByLabelText("Notify me"), { target: { value: "all_decisions" } });
  expect(onNotificationPolicyChange).toHaveBeenCalledWith("all_decisions");
  fireEvent.click(screen.getByRole("button", { name: "Enable this device" }));
  expect(onEnableNotifications).toHaveBeenCalledOnce();
  expect(screen.getAllByText("Healthy · 0.1.0").length).toBeGreaterThan(0);
  expect(screen.getByText("Meadow Hive")).toBeInTheDocument();
  expect(screen.getByText("Bea")).toBeInTheDocument();
  expect(screen.getByText("Personal Hive")).toBeInTheDocument();
  expect(await screen.findByText("Jira not connected")).toBeInTheDocument();
  expect(screen.getByText("Owned tasks continue; new shared claims wait")).toBeInTheDocument();
  expect(screen.getByText("Live updates").parentElement).toHaveTextContent("Live updatesConnected");
  expect(screen.getByText("Running workers").parentElement).toHaveTextContent("Running workers1");
  expect(screen.getByText("Retained sessions").parentElement).toHaveTextContent("Retained sessions3");
  expect((await screen.findByText("Worker engine")).parentElement).toHaveTextContent("Worker engineUpdate waiting · 1 active");
  fireEvent.click(screen.getByRole("button", { name: "Prepare worker engine update" }));
  expect(screen.getByRole("group", { name: "Confirm worker engine update" })).toHaveTextContent("Restart 1 active worker now?");
  fireEvent.click(screen.getByRole("button", { name: "Restart and update" }));
  expect(onUpdateWorkerEngine).toHaveBeenCalledOnce();
  const terminalHost = await screen.findByText("Terminal host");
  expect(terminalHost.parentElement).toHaveTextContent("Terminal hostHealthy · 0.1.0-host");
  expect((await screen.findByText("API memory")).parentElement).toHaveTextContent("API memoryNormal · 18.0 MiB");
  expect(screen.getByText("Terminal memory").parentElement).toHaveTextContent("Terminal memoryNormal · 9.0 MiB");
  expect(screen.getByText("Needs you").parentElement).toHaveTextContent("Needs youAlt1");
  expect(screen.getByText("Tasks").parentElement).toHaveTextContent("TasksAlt2");
  expect(screen.getByText("Workers").parentElement).toHaveTextContent("WorkersAlt3");
  expect(screen.getByText("Settings").parentElement).toHaveTextContent("SettingsAlt4");
  expect(screen.getByText("Quick navigation").parentElement).toHaveTextContent("Quick navigationAltK");
  const savedReportSummary = await screen.findByText("Terminal wrapped too narrowly", { selector: "summary span" });
  expect(savedReportSummary).toBeInTheDocument();
  fireEvent.click(savedReportSummary);
  expect(screen.getByText("Worker remains readable")).toBeInTheDocument();
  expect(screen.getByText("Attached privately")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Download screenshot" }));
  await vi.waitFor(() => expect(createObjectURL).toHaveBeenCalledOnce());
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

test("downloads a consistent Hive database snapshot", async () => {
  const createObjectURL = vi.fn().mockReturnValue("blob:hive-backup");
  const revokeObjectURL = vi.fn();
  vi.stubGlobal("URL", { ...URL, createObjectURL, revokeObjectURL });
  const click = vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => undefined);
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL) => {
    const url = String(input);
    if (url.includes("integrations/jira/readiness")) return ok({ configured: false, connection: "not_connected", account_name: null });
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

  render(<SettingsWorkspace {...minimalProps()} />);
  fireEvent.click(screen.getByRole("button", { name: "Download Hive backup" }));

  await vi.waitFor(() => expect(createObjectURL).toHaveBeenCalledOnce());
  expect(click).toHaveBeenCalledOnce();
  expect(revokeObjectURL).toHaveBeenCalledWith("blob:hive-backup");
});

function minimalProps() {
  return {
    busy: false, colorTheme: "light" as const, feedbackRevision: 0, liveFeedState: "connected" as const, operatorToken: "secret-token",
    presence: { mode: "at_hive" as const, manual_mode: null, source: "active_device" as const }, lockDetectionState: "unsupported" as const,
    notificationSettings: { policy: "important_only" as const, subscription_count: 0, vapid_public_key: "public-key" }, queenPolicy: { at_hive: "coordinate" as const, away: "coordinate" as const, night_watch: "local_execution" as const }, notificationState: "available" as const,
    recentEvents: [], sessions: [], workers: [], workspaces: [], providers: { claude_code: true, codex: false }, health: { status: "ok" as const, version: "0.1.0" },
    hiveIdentity: { operator: { id: "operator-1", display_name: "Bea" }, hive: { id: "hive-1", name: "Meadow Hive", operator_id: "operator-1", apiary_id: null } },
    onThemeChange: vi.fn(), onPresenceChange: vi.fn(), onEnableLockDetection: vi.fn(), onNotificationPolicyChange: vi.fn(),
    onQueenPolicyChange: vi.fn(), onEnableNotifications: vi.fn(), onDisableNotifications: vi.fn(), onTestNotification: vi.fn(), onCreateWorker: vi.fn(), onUpdateWorker: vi.fn(), onReorderWorkers: vi.fn(), onUpdateWorkerEngine: vi.fn(),
  };
}
function ok(body: unknown) {
  return new Response(JSON.stringify(body), { status: 200, headers: { "Content-Type": "application/json" } });
}

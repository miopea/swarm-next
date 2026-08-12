import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import SettingsWorkspace from "./SettingsWorkspace";

afterEach(() => vi.unstubAllGlobals());

test("shows subsystem diagnostics, previews a sanitized report, and changes the selected theme", async () => {
  const onThemeChange = vi.fn();
  const onPresenceChange = vi.fn().mockResolvedValue(undefined);
  const onEnableLockDetection = vi.fn().mockResolvedValue(undefined);
  const onNotificationPolicyChange = vi.fn().mockResolvedValue(undefined);
  const onEnableNotifications = vi.fn().mockResolvedValue(undefined);
  const onDisableNotifications = vi.fn().mockResolvedValue(undefined);
  const onTestNotification = vi.fn().mockResolvedValue(undefined);
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL) => {
    const url = String(input);
    if (url.includes("terminal-host")) return ok({ type: "host_status", status: { protocol_version: 5, host_version: "0.1.0", draining: false, running_sessions: 1, retained_sessions: 3 } });
    return ok({ type: "history_diagnostics", diagnostics: { retained_bytes: 42, session_count: 3, segment_count: 1, dropped_records: 0, dropped_bytes: 0, recovered_truncated_bytes: 0, recovered_corrupt_segments: 0 } });
  }));
  render(
    <SettingsWorkspace
      colorTheme="light"
      liveFeedState="connected"
      operatorToken="secret-token"
      presence={{ mode: "away", manual_mode: null, source: "screen_locked" }}
      lockDetectionState="available"
      notificationSettings={{ policy: "important_only", subscription_count: 0, vapid_public_key: "public-key" }}
      notificationState="available"
      recentEvents={[{ sequence: 7, hive_id: "hive-1", kind: "workers_changed", occurred_at: 1 }]}
      hiveIdentity={{ operator: { id: "operator-1", display_name: "Bea" }, hive: { id: "hive-1", name: "Meadow Hive", operator_id: "operator-1", apiary_id: null } }}
      health={{ status: "ok", version: "0.1.0" }}
      sessions={[{ session_id: "session-safe-id", running: true }, { session_id: "session-2", running: false }, { session_id: "session-3", running: false }]}
      workers={[{ id: "worker-1", hive_id: "hive-1", name: "Private name", role: "worker", provider: "claude_code", workspace: "/private/workspace", autostart: false, position: 1, active_session_id: "session-safe-id", created_at: 1, updated_at: 1, running: true, attention_state: "blocked", runtime_error: "raw provider failure detail" }]}
      onThemeChange={onThemeChange}
      onPresenceChange={onPresenceChange}
      onEnableLockDetection={onEnableLockDetection}
      onNotificationPolicyChange={onNotificationPolicyChange}
      onEnableNotifications={onEnableNotifications}
      onDisableNotifications={onDisableNotifications}
      onTestNotification={onTestNotification}
    />,
  );

  expect(screen.getAllByRole("status")[0]).toHaveTextContent("AwayComputer lock detected");
  fireEvent.change(screen.getByLabelText("Presence policy"), { target: { value: "night_watch" } });
  expect(onPresenceChange).toHaveBeenCalledWith("night_watch");
  fireEvent.click(screen.getByRole("button", { name: "Enable" }));
  expect(onEnableLockDetection).toHaveBeenCalledOnce();
  expect(screen.getByText("Available when you choose")).toBeInTheDocument();
  fireEvent.change(screen.getByLabelText("Notify me"), { target: { value: "all_decisions" } });
  expect(onNotificationPolicyChange).toHaveBeenCalledWith("all_decisions");
  fireEvent.click(screen.getByRole("button", { name: "Enable this device" }));
  expect(onEnableNotifications).toHaveBeenCalledOnce();
  expect(screen.getAllByText("Healthy · 0.1.0").length).toBeGreaterThan(0);
  expect(screen.getByText("Meadow Hive")).toBeInTheDocument();
  expect(screen.getByText("Bea")).toBeInTheDocument();
  expect(screen.getByText("Personal Hive")).toBeInTheDocument();
  expect(screen.getByText("Live updates").parentElement).toHaveTextContent("Live updatesConnected");
  expect(screen.getByText("Running workers").parentElement).toHaveTextContent("Running workers1");
  expect(screen.getByText("Retained sessions").parentElement).toHaveTextContent("Retained sessions3");
  const terminalHost = await screen.findByText("Terminal host");
  expect(terminalHost.parentElement).toHaveTextContent("Terminal hostHealthy · 0.1.0");

  fireEvent.click(screen.getByRole("button", { name: "Preview report" }));
  const preview = screen.getByLabelText("Sanitized diagnostic report");
  expect(preview).toHaveTextContent("session-safe-id");
  expect(preview).toHaveTextContent("workers_changed");
  expect(preview).not.toHaveTextContent("Private name");
  expect(preview).not.toHaveTextContent("/private/workspace");
  expect(preview).not.toHaveTextContent("secret-token");
  expect(preview).not.toHaveTextContent("raw provider failure detail");

  expect(screen.getByRole("button", { name: "Light meadow" })).toHaveAttribute("aria-pressed", "true");
  fireEvent.click(screen.getByRole("button", { name: "Night hive" }));
  expect(onThemeChange).toHaveBeenCalledWith("dark");
});

function ok(body: unknown) {
  return new Response(JSON.stringify(body), { status: 200, headers: { "Content-Type": "application/json" } });
}

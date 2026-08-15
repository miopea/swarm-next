import { afterEach, expect, test } from "vitest";

import { clearSettingsSection, navigateToSettingsSection, readSettingsSection } from "./settingsNavigation";

afterEach(() => window.history.replaceState({}, "", "/"));

test("recognizes every stable settings anchor and rejects unrelated hashes", () => {
  expect(readSettingsSection("#settings-apiary", "")).toBe("settings-apiary");
  expect(readSettingsSection("#settings-diagnostics", "")).toBe("settings-diagnostics");
  expect(readSettingsSection("#worker-1", "")).toBeUndefined();
});

test("routes Jira callbacks to integrations when no explicit anchor is present", () => {
  expect(readSettingsSection("", "?jira=connected")).toBe("settings-integrations");
  expect(readSettingsSection("#settings-apiary", "?jira=connected")).toBe("settings-apiary");
});

test("writes durable section anchors and clears them outside settings", () => {
  navigateToSettingsSection("settings-crew");
  expect(window.location.hash).toBe("#settings-crew");
  clearSettingsSection();
  expect(window.location.hash).toBe("");
});

import { afterEach, expect, test } from "vitest";

import {
  SETTINGS_CARDS,
  SETTINGS_SECTIONS,
  clearSettingsSection,
  filterSettingsCards,
  navigateToSettingsSection,
  readSettingsSection,
} from "./settingsNavigation";

afterEach(() => window.history.replaceState({}, "", "/"));

test("recognizes every section anchor and rejects unrelated hashes", () => {
  expect(readSettingsSection("#settings-connections", "")).toBe("settings-connections");
  expect(readSettingsSection("#settings-maintenance", "")).toBe("settings-maintenance");
  expect(readSettingsSection("#worker-1", "")).toBeUndefined();
});

/**
 * Anchors are shared: the operator bookmarks them, other surfaces link to
 * them, and a Jira hand-off arrives at one. Regrouping must not turn any of
 * those into a page that opens nowhere.
 */
test("an older anchor still opens the section that now holds it", () => {
  expect(readSettingsSection("#settings-crew", "")).toBe("settings-workers");
  expect(readSettingsSection("#settings-queen", "")).toBe("settings-workers");
  expect(readSettingsSection("#settings-apiary", "")).toBe("settings-connections");
  expect(readSettingsSection("#settings-runtime", "")).toBe("settings-updates");
  expect(readSettingsSection("#settings-diagnostics", "")).toBe("settings-maintenance");
  expect(readSettingsSection("#settings-notifications", "")).toBe("settings-hive");
});

test("routes Jira callbacks to connections when no explicit anchor is present", () => {
  expect(readSettingsSection("", "?jira=connected")).toBe("settings-connections");
  expect(readSettingsSection("#settings-maintenance", "?jira=connected")).toBe("settings-maintenance");
});

test("writes durable section anchors and clears them outside settings", () => {
  navigateToSettingsSection("settings-access");
  expect(window.location.hash).toBe("#settings-access");
  clearSettingsSection();
  expect(window.location.hash).toBe("");
});

/**
 * The defect that started this: four cards were rendered into the page and
 * belonged to no section, so the operator went looking for a passkey and for
 * the tunnel and found neither.
 */
test("every card belongs to a real section", () => {
  const sections = new Set(SETTINGS_SECTIONS.map(([id]) => id));
  for (const card of SETTINGS_CARDS) {
    expect(sections.has(card.section), `${card.id} is in no section`).toBe(true);
  }
  expect(SETTINGS_CARDS.map((card) => card.id)).toContain("settings-access");
  expect(SETTINGS_CARDS.map((card) => card.id)).toContain("settings-remote");
  expect(SETTINGS_CARDS.map((card) => card.id)).toContain("settings-appearance");
  expect(SETTINGS_CARDS.map((card) => card.id)).toContain("settings-email");
});

test("no two cards claim the same anchor", () => {
  const ids = SETTINGS_CARDS.map((card) => card.id);
  expect(new Set(ids).size).toBe(ids.length);
});

test("finds a control by the word someone would actually type", () => {
  const titles = (query: string) => filterSettingsCards(query).map((card) => card.title);

  expect(titles("passkey")).toEqual(["Operator access"]);
  expect(titles("tunnel")).toEqual(["Open on my phone"]);
  expect(titles("qr")).toEqual(["Open on my phone"]);
  expect(titles("token")).toEqual(["Operator access"]);
  expect(titles("outlook")).toEqual(["Email"]);
  expect(titles("upgrade")).toEqual(["App and API"]);
});

test("matches the section name too, so browsing by group still works", () => {
  expect(filterSettingsCards("access").map((card) => card.id))
    .toEqual(["settings-access", "settings-remote"]);
});

test("an empty query is a way to jump, not a way to browse", () => {
  expect(filterSettingsCards("")).toEqual([]);
  expect(filterSettingsCards("   ")).toEqual([]);
});

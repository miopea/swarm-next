import { afterEach, expect, test } from "vitest";

import {
  SETTINGS_CARDS,
  SETTINGS_SECTIONS,
  clearSettingsSection,
  filterSettingsCards,
  navigateToSettingsSection,
  readSettingsSection,
  visibleSettingsSections,
} from "./settingsNavigation";

afterEach(() => window.history.replaceState({}, "", "/"));

test("developer evidence is discoverable only in the existing development mode", () => {
  expect(visibleSettingsSections(false).map(([id]) => id)).not.toContain("settings-dogfood");
  expect(visibleSettingsSections(true).map(([id]) => id)).toContain("settings-dogfood");
  expect(filterSettingsCards("dogfood")).toEqual([]);
  expect(filterSettingsCards("dogfood", true).map((card) => card.id)).toEqual(["settings-dogfood"]);
});

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
  // The rules about what happens without the operator moved to Authority.
  expect(readSettingsSection("#settings-queen", "")).toBe("settings-authority");
  expect(readSettingsSection("#settings-queen-releases", "")).toBe("settings-authority");
  expect(readSettingsSection("#settings-check-gate", "")).toBe("settings-authority");
  expect(readSettingsSection("#settings-answers", "")).toBe("settings-authority");
  // Provider usage is what workers spend, not upkeep.
  expect(readSettingsSection("#settings-usage", "")).toBe("settings-workers");
  // Apiary moved to Your Hive when joining was simplified; an old bookmark must
  // still land where the card actually is, which is the whole point of this test.
  expect(readSettingsSection("#settings-apiary", "")).toBe("settings-hive");
  expect(readSettingsSection("#settings-runtime", "")).toBe("settings-updates");
  expect(readSettingsSection("#settings-diagnostics", "")).toBe("settings-maintenance");
  // The operator's own preferences left Your Hive for You.
  expect(readSettingsSection("#settings-notifications", "")).toBe("settings-you");
  expect(readSettingsSection("#settings-presence", "")).toBe("settings-you");
  expect(readSettingsSection("#settings-shortcuts", "")).toBe("settings-you");
  expect(readSettingsSection("#settings-system", "")).toBe("settings-updates");
  // The sections themselves keep their anchors.
  expect(readSettingsSection("#settings-hive", "")).toBe("settings-hive");
  expect(readSettingsSection("#settings-workers", "")).toBe("settings-workers");
});

/**
 * Every rule about what happens without the operator, in one section.
 *
 * They were split between Workers, below the roster, and Your Hive, and which
 * workers held raised access could only be read row by row on the roster.
 */
test("Authority holds every rule about what happens without the operator", () => {
  const authority = SETTINGS_CARDS.filter((card) => card.section === "settings-authority").map((card) => card.id);
  expect(authority).toEqual(expect.arrayContaining([
    "settings-queen", "settings-queen-releases", "settings-check-gate", "settings-answers", "settings-worker-access",
  ]));
  expect(authority).toHaveLength(5);
  expect(filterSettingsCards("authority").map((card) => card.id).sort()).toEqual([...authority].sort());
  expect(SETTINGS_CARDS.filter((card) => card.section === "settings-hive").map((card) => card.id))
    .toEqual(["settings-identity", "settings-apiary"]);
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
  expect(SETTINGS_CARDS.find((card) => card.id === "settings-identity")?.section).toBe("settings-hive");
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
  // ⚠️ "token" MEANS TWO THINGS IN THIS APP and the filter is right to show
  // both: the operator's own credential, and the unit a provider bills in.
  // Narrowing either card's keywords to disambiguate would hide the one the
  // reader wanted from whichever of the two they did not think to spell out.
  expect(titles("token")).toEqual(["Operator access", "Provider usage"]);
  expect(titles("operator token")).toEqual(["Operator access"]);
  expect(titles("token spend")).toEqual(["Provider usage"]);
  expect(titles("outlook")).toEqual(["Email"]);
  expect(titles("upgrade")).toEqual(["App and API"]);
  expect(titles("membership")).toEqual(["Your Hive"]);
});

/**
 * Other operators learn about "Ask me before resolving tickets" from the
 * release notes and then search for it in their own words. "resolve" also
 * belongs to Terminal answers, and both are right to show.
 */
test("finds the Jira resolution setting by its label and by asking for approval", () => {
  const titles = (query: string) => filterSettingsCards(query).map((card) => card.title);

  expect(titles("ask me before resolving tickets")).toEqual(["Jira"]);
  expect(titles("resolving")).toEqual(["Jira"]);
  expect(titles("resolve")).toEqual(["Terminal answers", "Jira"]);
  expect(titles("approve")).toEqual(["Jira"]);
  expect(titles("approval")).toEqual(["Jira"]);
});

test("finds the Remote Control setting by its name and by /rc", () => {
  const titles = (query: string) => filterSettingsCards(query).map((card) => card.title);

  expect(titles("remote control")).toEqual(["Remote Control"]);
  expect(titles("/rc")).toEqual(["Remote Control"]);
});

/** ADR 0131: an operator whose merges keep being refused looks for it in those words. */
test("finds the Queen releases setting by merge, deploy and the refusal", () => {
  const titles = (query: string) => filterSettingsCards(query).map((card) => card.title);

  expect(titles("queen releases")).toEqual(["Queen releases"]);
  expect(titles("merge")).toEqual(["Queen releases"]);
  expect(titles("deploy")).toEqual(["Queen releases"]);
  expect(titles("permission denied")).toEqual(["Queen releases"]);
});

test("matches the section name too, so browsing by group still works", () => {
  // "Worker access" matches by its own title; the other two by their section.
  expect(filterSettingsCards("access").map((card) => card.id))
    .toEqual(["settings-worker-access", "settings-access", "settings-remote"]);
});

test("finds visible settings labels and multi-word queries without requiring a literal phrase", () => {
  const ids = (query: string, development = false) => filterSettingsCards(query, development).map((card) => card.id);
  expect(ids("night watch")).toContain("settings-presence");
  expect(ids("night watch schedule")).toEqual(["settings-presence"]);
  expect(ids(" SCHEDULE   night WATCH ")).toEqual(["settings-presence"]);
  expect(ids("opening screen")).toEqual(["settings-presence"]);
  expect(ids("time zone")).toEqual(["settings-presence"]);
  expect(ids("dark theme")).toEqual(["settings-appearance"]);
  expect(ids("notifications")).toEqual(["settings-notifications"]);
  expect(ids("notify me")).toEqual(["settings-notifications"]);
  expect(ids("phone notifications")).toEqual(["settings-notifications"]);
  expect(ids("passkey schedule")).toEqual([]);
  expect(ids("developer evidence")).toEqual([]);
  expect(ids("developer evidence", true)).toEqual(["settings-dogfood"]);
});

test("an empty query is a way to jump, not a way to browse", () => {
  expect(filterSettingsCards("")).toEqual([]);
  expect(filterSettingsCards("   ")).toEqual([]);
});

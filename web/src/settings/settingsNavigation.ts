export const SETTINGS_SECTIONS = [
  ["settings-crew", "Crew"],
  ["settings-presence", "Presence"],
  ["settings-queen", "Queen"],
  ["settings-notifications", "Alerts"],
  ["settings-runtime", "System"],
  ["settings-apiary", "Apiary"],
  ["settings-integrations", "Integrations"],
  ["settings-migration", "Migration"],
  ["settings-backup", "Backup"],
  ["settings-diagnostics", "Diagnostics"],
] as const;

export type SettingsSection = (typeof SETTINGS_SECTIONS)[number][0];

const SETTINGS_SECTION_IDS = new Set<string>(SETTINGS_SECTIONS.map(([id]) => id));

export function readSettingsSection(
  hash = window.location.hash,
  search = window.location.search,
): SettingsSection | undefined {
  const id = hash.startsWith("#") ? hash.slice(1) : hash;
  if (SETTINGS_SECTION_IDS.has(id)) return id as SettingsSection;
  return new URLSearchParams(search).has("jira") ? "settings-integrations" : undefined;
}

export function navigateToSettingsSection(section: SettingsSection): void {
  if (window.location.hash === `#${section}`) return;
  window.location.hash = section;
}

export function clearSettingsSection(): void {
  if (!readSettingsSection()) return;
  window.history.replaceState(window.history.state, "", `${window.location.pathname}${window.location.search}`);
}

/**
 * What is in Settings, and where to find it.
 *
 * Grouped by what the operator is trying to do rather than by which subsystem
 * owns the control. The previous grouping had grown to ten sections named after
 * internals — "Crew", "System", "Integrations" — while four cards were in the
 * page and in no section at all: Appearance, Access, Remote access and Email.
 * Two of those are the ones the operator went looking for and could not find.
 */
export const SETTINGS_SECTIONS = [
  ["settings-hive", "Your Hive"],
  ["settings-workers", "Workers"],
  ["settings-access", "Access"],
  ["settings-connections", "Connections"],
  ["settings-updates", "Updates"],
  ["settings-maintenance", "Maintenance"],
] as const;

export type SettingsSection = (typeof SETTINGS_SECTIONS)[number][0];

/**
 * Every card, the section it lives in, and the words someone would type looking
 * for it.
 *
 * The keywords are the index the filter searches. They are deliberately the
 * operator's words rather than the code's: nobody searches for "runtime" when
 * they want to know whether an update is available.
 */
export type SettingsCard = {
  id: string;
  section: SettingsSection;
  title: string;
  keywords: readonly string[];
};

export const SETTINGS_CARDS: readonly SettingsCard[] = [
  { id: "settings-crew", section: "settings-workers", title: "Crew",
    keywords: ["worker", "crew", "add worker", "repository", "workspace", "provider", "claude", "codex", "description", "order", "remove worker"] },
  { id: "settings-queen", section: "settings-workers", title: "Queen",
    keywords: ["queen", "autonomy", "ceiling", "coordinate", "night watch", "review", "automation"] },
  { id: "settings-presence", section: "settings-hive", title: "Presence",
    keywords: ["presence", "away", "at hive", "night", "lock", "screen lock", "asleep"] },
  { id: "settings-appearance", section: "settings-hive", title: "Appearance",
    keywords: ["appearance", "theme", "dark", "light", "colour", "color", "terminal keys", "keyboard"] },
  { id: "settings-notifications", section: "settings-hive", title: "Alerts",
    keywords: ["alerts", "notification", "push", "subscribe", "phone", "quiet"] },
  { id: "settings-access", section: "settings-access", title: "Operator access",
    keywords: ["access", "token", "operator token", "passkey", "password", "sign in", "sign out", "rotate", "credential", "webauthn", "unlock", "security"] },
  { id: "settings-remote", section: "settings-access", title: "Open on my phone",
    keywords: ["remote", "tunnel", "phone", "qr", "qr code", "cloudflare", "cloudflared", "public address", "share", "mobile", "away from my desk"] },
  { id: "settings-apiary", section: "settings-connections", title: "Apiary",
    keywords: ["apiary", "hive name", "identity", "keeper", "member", "join", "federation", "invite"] },
  { id: "settings-integrations", section: "settings-connections", title: "Jira",
    keywords: ["jira", "atlassian", "project", "issue", "board", "ticket"] },
  { id: "settings-email", section: "settings-connections", title: "Email",
    keywords: ["email", "outlook", "microsoft", "mailbox", "reply", "oauth", "inbox"] },
  { id: "settings-runtime", section: "settings-updates", title: "App and API",
    keywords: ["update", "release", "version", "upgrade", "install", "worker engine", "development", "reload", "build", "provider", "restart", "system"] },
  { id: "settings-backup", section: "settings-maintenance", title: "Backup",
    keywords: ["backup", "export", "restore", "snapshot", "copy", "carry"] },
  { id: "settings-migration", section: "settings-maintenance", title: "Migration",
    keywords: ["migration", "legacy", "import", "old swarm", "move"] },
  { id: "settings-shortcuts", section: "settings-hive", title: "Keyboard",
    keywords: ["keyboard", "shortcut", "shortcuts", "alt", "hotkey", "quick navigation"] },
  { id: "settings-diagnostics", section: "settings-maintenance", title: "Diagnostics",
    keywords: ["diagnostics", "logs", "history", "terminal history", "debug", "support"] },
];

const SETTINGS_SECTION_IDS = new Set<string>(SETTINGS_SECTIONS.map(([id]) => id));

/**
 * Where an older link lands now.
 *
 * Anchors are shared — the operator bookmarks them, other surfaces link to
 * them, and a Jira hand-off arrives at one. Regrouping must not turn any of
 * those into a page that opens nowhere, so a card id resolves to its section
 * and the ids that were only ever sections resolve too.
 */
const SETTINGS_SECTION_ALIASES = new Map<string, SettingsSection>([
  ...SETTINGS_CARDS.map((card) => [card.id, card.section] as const),
  ["settings-notifications", "settings-hive"],
  ["settings-runtime", "settings-updates"],
  ["settings-system", "settings-updates"],
]);

export function resolveSettingsSection(id: string): SettingsSection | undefined {
  if (SETTINGS_SECTION_IDS.has(id)) return id as SettingsSection;
  return SETTINGS_SECTION_ALIASES.get(id);
}

export function readSettingsSection(
  hash = window.location.hash,
  search = window.location.search,
): SettingsSection | undefined {
  const id = hash.startsWith("#") ? hash.slice(1) : hash;
  const resolved = resolveSettingsSection(id);
  if (resolved) return resolved;
  return new URLSearchParams(search).has("jira") ? "settings-connections" : undefined;
}

/**
 * The cards matching what the operator typed, in page order.
 *
 * Matches the card's title, its section's name and its keywords, so "phone"
 * finds both the tunnel and alerts, and "token" finds access. An empty query
 * matches nothing: the filter is a way to jump, not a way to browse.
 */
export function filterSettingsCards(query: string): readonly SettingsCard[] {
  const needle = query.trim().toLowerCase();
  if (!needle) return [];
  const sectionLabel = new Map<string, string>(SETTINGS_SECTIONS.map(([id, label]) => [id, label.toLowerCase()]));
  return SETTINGS_CARDS.filter((card) =>
    card.title.toLowerCase().includes(needle)
    || (sectionLabel.get(card.section) ?? "").includes(needle)
    || card.keywords.some((keyword) => keyword.includes(needle)));
}

export function sectionLabel(section: SettingsSection): string {
  return SETTINGS_SECTIONS.find(([id]) => id === section)?.[1] ?? "Settings";
}

export function navigateToSettingsSection(section: SettingsSection): void {
  if (window.location.hash === `#${section}`) return;
  window.location.hash = section;
}

export function clearSettingsSection(): void {
  if (!readSettingsSection()) return;
  window.history.replaceState(window.history.state, "", `${window.location.pathname}${window.location.search}`);
}

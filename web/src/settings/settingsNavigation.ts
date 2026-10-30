/**
 * What is in Settings, and where to find it.
 *
 * Grouped by what the operator is trying to do rather than by which subsystem
 * owns the control. The previous grouping had grown to ten sections named after
 * internals — "Crew", "System", "Integrations" — while four cards were in the
 * page and in no section at all: Appearance, Access, Remote access and Email.
 * Two of those are the ones the operator went looking for and could not find.
 *
 * ⚠️ "INTEGRATIONS" IS BACK AS A NAME, AND IT IS NOT THE SECTION THAT WAS
 * REMOVED. The old one was a subsystem bucket. This one holds exactly what a
 * person means by the word: the outside services this Hive connects to — MCP
 * tools, Jira, Email. What made the difference is that Apiary LEFT: an Apiary is
 * not a service you integrate with, it is who this Hive is federated with, so it
 * moved to "Your Hive" beside identity. Once it was gone, "Connections" was the
 * vaguer word for what remained.
 *
 * ⚠️ AUTHORITY HOLDS EVERY RULE ABOUT WHAT HAPPENS WITHOUT THE OPERATOR. Queen
 * autonomy, Queen releases and the check gate sat under Workers below the
 * roster, Terminal answers sat under Your Hive, and which workers held raised
 * access could only be read row by row on the roster. Those are the settings
 * behind a week of refused merges and the Queen releases mix-up, and nothing
 * showed them together. Your Hive had become personal and device preferences
 * beside the Hive's identity; the preferences are "You" now. The roster section
 * stays "Workers" rather than "Crew", the internal name this grouping replaced.
 */
export const SETTINGS_SECTIONS = [
  ["settings-hive", "Your Hive"],
  ["settings-you", "You"],
  ["settings-workers", "Workers"],
  ["settings-authority", "Authority"],
  ["settings-access", "Access"],
  ["settings-connections", "Integrations"],
  ["settings-updates", "Updates & restarts"],
  ["settings-maintenance", "Maintenance"],
  ["settings-dogfood", "Developer Dogfood"],
] as const;

export type SettingsSection = (typeof SETTINGS_SECTIONS)[number][0];

export function visibleSettingsSections(developmentMode: boolean) {
  return SETTINGS_SECTIONS.filter(([id]) => developmentMode || id !== "settings-dogfood");
}

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
  { id: "settings-identity", section: "settings-hive", title: "Your Hive",
    keywords: ["identity", "hive name", "operator", "membership"] },
  { id: "settings-dogfood", section: "settings-dogfood", title: "Developer Dogfood",
    keywords: ["developer", "dogfood", "performance", "soak", "evidence", "regression"] },
  { id: "settings-crew", section: "settings-workers", title: "Crew",
    keywords: ["worker", "crew", "add worker", "repository", "workspace", "provider", "claude", "codex", "description", "order", "remove worker"] },
  { id: "settings-queen", section: "settings-authority", title: "Queen",
    keywords: ["queen", "autonomy", "ceiling", "coordinate", "night watch", "review", "automation"] },
  { id: "settings-check-gate", section: "settings-authority", title: "Check gate",
    keywords: ["check gate", "ci", "checks", "github", "actions", "strictness", "require", "passing", "done", "close", "review"] },
  { id: "settings-queen-releases", section: "settings-authority", title: "Queen releases",
    keywords: ["queen releases", "release", "merge", "deploy", "ship", "auto mode", "refused", "permission denied"] },
  // Read-only. Each worker's access is still set in its own editor on the
  // roster; this is the one place that shows who holds any.
  { id: "settings-worker-access", section: "settings-authority", title: "Worker access",
    keywords: ["worker access", "system access", "commands", "sudo", "without asking", "read the whole board", "board", "privilege", "permissions", "audit"] },
  { id: "settings-worker-model", section: "settings-workers", title: "Worker model",
    keywords: ["worker model", "model", "opus", "sonnet", "haiku", "claude", "cost", "cheaper"] },
  { id: "settings-worker-remote-control", section: "settings-workers", title: "Remote Control",
    keywords: ["remote control", "/rc", "claude app", "claude.ai", "follow", "restart", "update", "new version"] },
  { id: "settings-answers", section: "settings-authority", title: "Terminal answers",
    keywords: ["terminal answers", "needs you", "native", "answer", "resolve", "decision", "terminal", "kill switch", "off"] },
  { id: "settings-presence", section: "settings-you", title: "Presence",
    keywords: ["presence", "away", "reachable", "automatic", "at hive", "night watch", "schedule", "daily", "time zone", "starts", "ends", "opening screen", "lock", "screen lock", "asleep"] },
  { id: "settings-appearance", section: "settings-you", title: "Appearance",
    keywords: ["appearance", "theme", "dark", "light", "colour", "color", "terminal keys", "keyboard"] },
  { id: "settings-notifications", section: "settings-you", title: "Alerts",
    keywords: ["alerts", "notifications", "notify me", "push", "subscribe", "phone", "device", "quiet"] },
  { id: "settings-access", section: "settings-access", title: "Operator access",
    keywords: ["access", "token", "operator token", "passkey", "password", "sign in", "sign out", "rotate", "credential", "webauthn", "unlock", "security"] },
  { id: "settings-remote", section: "settings-access", title: "Open on my phone",
    keywords: ["remote", "tunnel", "phone", "qr", "qr code", "cloudflare", "cloudflared", "public address", "share", "mobile", "away from my desk"] },
  { id: "settings-connections-tools", section: "settings-connections", title: "Outside tools",
    keywords: ["mcp", "connector", "outside tool", "claude desktop", "external", "oauth", "connection", "disconnect", "revoke"] },
  // MOVED OUT OF INTEGRATIONS, and deliberately. An Apiary is not an outside
  // tool this Hive connects to — it is who this Hive is federated with, which is
  // the same question "Your Hive" already answers. It sat under Connections when
  // joining was an involved setup; that has been simplified, so it belongs with
  // identity. "connections" and "integrations" stay in the keywords so anyone who
  // learned the old location still finds it.
  { id: "settings-apiary", section: "settings-hive", title: "Apiary",
    keywords: ["apiary", "hive name", "identity", "keeper", "member", "join", "federation", "invite", "connections", "integrations"] },
  { id: "settings-integrations", section: "settings-connections", title: "Jira",
    keywords: ["jira", "atlassian", "project", "issue", "board", "ticket", "ask me before resolving tickets", "resolved", "approved", "approval", "confirm"] },
  { id: "settings-email", section: "settings-connections", title: "Email",
    keywords: ["email", "outlook", "microsoft", "mailbox", "reply", "oauth", "inbox"] },
  { id: "settings-usage", section: "settings-workers", title: "Provider usage",
    keywords: ["usage", "tokens", "token", "spend", "spending", "burn", "cost", "cache", "cached", "context", "budget", "quota", "provider usage"] },
  { id: "settings-runtime", section: "settings-updates", title: "App and API",
    keywords: ["update", "release", "version", "upgrade", "install", "worker engine", "development", "reload", "build", "provider", "restart", "system"] },
  { id: "settings-terminal-history", section: "settings-maintenance", title: "Terminal history",
    keywords: ["terminal history", "retention", "keep", "days", "scrollback", "disk", "storage", "delete"] },
  { id: "settings-backup", section: "settings-maintenance", title: "Backup",
    keywords: ["backup", "export", "restore", "snapshot", "copy", "carry"] },
  { id: "settings-migration", section: "settings-maintenance", title: "Migration",
    keywords: ["migration", "legacy", "import", "old swarm", "move"] },
  { id: "settings-shortcuts", section: "settings-you", title: "Keyboard",
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
 * matches nothing: the filter is a way to jump, not a way to browse. Every
 * query word must match the same card, but may come from different labels.
 */
export function filterSettingsCards(query: string, developmentMode = false): readonly SettingsCard[] {
  const words = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
  if (!words.length) return [];
  const sectionLabel = new Map<string, string>(SETTINGS_SECTIONS.map(([id, label]) => [id, label.toLowerCase()]));
  return SETTINGS_CARDS.filter((card) => developmentMode || card.id !== "settings-dogfood").filter((card) => {
    const searchable = [card.title, sectionLabel.get(card.section) ?? "", ...card.keywords]
      .join(" ").toLowerCase();
    return words.every((word) => searchable.includes(word));
  });
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

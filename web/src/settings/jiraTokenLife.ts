/**
 * How long an Atlassian API token has left, stated no more precisely than we
 * can honestly know.
 *
 * ⚠️ SWARM CANNOT LEARN A TOKEN'S EXPIRY. Basic auth carries none, and there is
 * no endpoint to ask. All we record is when this host was HANDED the token, and
 * the token may have been minted months earlier and pasted later. Atlassian
 * expires a token within a year of its CREATION, so connect time gives an upper
 * bound on the death date and nothing tighter: the token dies ON OR BEFORE
 * connectedAt + one year.
 *
 * Every string below says "on or before" for that reason. Saying "expires on"
 * would be a precision we do not have, and the first person to be told a wrong
 * date stops believing the next warning too.
 */

/** Atlassian's default and maximum life for tokens created after 2024-12-15. */
const TOKEN_LIFETIME_DAYS = 365;

/** How close to the bound before the notice becomes a warning. */
const WARN_WITHIN_DAYS = 30;

const DAY_SECONDS = 86_400;

export type JiraTokenLife =
  | { kind: "unknown" }
  | { kind: "fine"; expiresBy: Date }
  | { kind: "expiring"; expiresBy: Date; daysLeft: number }
  | { kind: "overdue"; expiresBy: Date };

export function jiraTokenLife(connectedAt: number | null | undefined, now: Date): JiraTokenLife {
  // Unknown is a real answer, not a zero. A connection made before Swarm
  // recorded dates must not be described as if it were made today.
  if (connectedAt === null || connectedAt === undefined || !Number.isFinite(connectedAt) || connectedAt <= 0) {
    return { kind: "unknown" };
  }
  const expiresBy = new Date((connectedAt + TOKEN_LIFETIME_DAYS * DAY_SECONDS) * 1000);
  const secondsLeft = (expiresBy.getTime() - now.getTime()) / 1000;
  if (secondsLeft <= 0) return { kind: "overdue", expiresBy };
  const daysLeft = Math.ceil(secondsLeft / DAY_SECONDS);
  if (daysLeft <= WARN_WITHIN_DAYS) return { kind: "expiring", expiresBy, daysLeft };
  return { kind: "fine", expiresBy };
}

export function jiraTokenLifeMessage(life: JiraTokenLife): string | null {
  const on = (date: Date) => date.toLocaleDateString(undefined, { year: "numeric", month: "long", day: "numeric" });
  switch (life.kind) {
    case "unknown":
      return null;
    case "fine":
      return `Atlassian API tokens expire within a year of being created, so this one stops working on or before ${on(life.expiresBy)}.`;
    case "expiring":
      return `This token expires on or before ${on(life.expiresBy)} — about ${life.daysLeft} ${life.daysLeft === 1 ? "day" : "days"} away. Create a new one and reconnect before it lapses; Jira will simply start refusing otherwise.`;
    case "overdue":
      return `This connection was made over a year ago, so its token has very likely expired — Atlassian tokens last at most a year. If Jira has stopped working, create a new token and reconnect.`;
  }
}

const FAILURE_KINDS = [
  "window_error", "unhandled_rejection", "react_render",
  "terminal_grant_timeout", "terminal_socket_open_timeout",
  "terminal_restore_timeout", "terminal_probe_timeout",
] as const;

export type ClientFailure = {
  kind: typeof FAILURE_KINDS[number];
  occurred_at: number;
};

const STORAGE_KEY = "swarm-next.client-failures.v1";
const MAX_FAILURES = 20;

export function recordClientFailure(kind: ClientFailure["kind"]) {
  const failures = [...readClientFailures(), { kind, occurred_at: Date.now() }].slice(-MAX_FAILURES);
  try {
    window.sessionStorage.setItem(STORAGE_KEY, JSON.stringify(failures));
  } catch {
    // Runtime recovery must not depend on browser storage availability.
  }
}

export function readClientFailures(): ClientFailure[] {
  try {
    const parsed = JSON.parse(window.sessionStorage.getItem(STORAGE_KEY) ?? "[]") as unknown;
    if (!Array.isArray(parsed)) return [];
    // Project storage onto the allowlist; extra fields must not enter reports.
    return parsed.filter(isClientFailure).slice(-MAX_FAILURES)
      .map(({ kind, occurred_at }) => ({ kind, occurred_at }));
  } catch {
    return [];
  }
}

export function installClientFailureCapture() {
  window.addEventListener("error", () => recordClientFailure("window_error"));
  window.addEventListener("unhandledrejection", () => recordClientFailure("unhandled_rejection"));
}

function isClientFailure(value: unknown): value is ClientFailure {
  if (!value || typeof value !== "object") return false;
  const candidate = value as Partial<ClientFailure>;
  return (FAILURE_KINDS as readonly unknown[]).includes(candidate.kind)
    && typeof candidate.occurred_at === "number"
    && Number.isFinite(candidate.occurred_at)
    && candidate.occurred_at >= 0;
}

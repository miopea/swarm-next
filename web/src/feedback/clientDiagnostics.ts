export type ClientFailure = {
  kind: "window_error" | "unhandled_rejection" | "react_render";
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
    return parsed.filter(isClientFailure).slice(-MAX_FAILURES);
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
  return (candidate.kind === "window_error" || candidate.kind === "unhandled_rejection" || candidate.kind === "react_render")
    && typeof candidate.occurred_at === "number";
}

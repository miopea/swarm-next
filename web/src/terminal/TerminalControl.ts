/** Engine facts only. Browser focus and timers never manufacture ownership. */
export interface TerminalControlStatus {
  readonly supported: boolean;
  readonly generation: string | null;
  readonly owned: boolean;
  readonly occupied: boolean;
  readonly lease_remaining_ms: number;
}

const MAX_GENERATION = 18_446_744_073_709_551_615n;
const UNCONFIRMED: TerminalControlStatus = Object.freeze({
  supported: false, generation: null, owned: false, occupied: false, lease_remaining_ms: 0,
});

function parseStatus(value: unknown): TerminalControlStatus | undefined {
  if (typeof value !== "object" || value === null) return;
  const candidate = value as Record<string, unknown>;
  const { supported, generation, owned, occupied, lease_remaining_ms: remaining } = candidate;
  if (typeof supported !== "boolean" || typeof owned !== "boolean" || typeof occupied !== "boolean"
    || typeof remaining !== "number" || !Number.isSafeInteger(remaining) || remaining < 0 || remaining > 300_000) return;
  if (owned && !occupied) return;
  if (!occupied && remaining !== 0) return;
  if (!supported) {
    if (generation !== null || owned || occupied || remaining !== 0) return;
  } else {
    if (typeof generation !== "string" || !/^(0|[1-9][0-9]{0,19})$/.test(generation)) return;
    if (BigInt(generation) > MAX_GENERATION || (generation === "0" && occupied)) return;
  }
  return Object.freeze({ supported, generation: generation as string | null, owned, occupied, lease_remaining_ms: remaining });
}

/**
 * One immutable engine session, one constant-space browser observation.
 * Output and command replies can race, so receipt order is not ownership order.
 * A disconnect removes permission locally without discarding the watermark.
 */
export class TerminalControl {
  #status = UNCONFIRMED;
  #confirmed = false;
  #generation: bigint | undefined;
  #expired = false;

  get status(): TerminalControlStatus { return this.#status; }
  get confirmed(): boolean { return this.#confirmed; }
  get ownsControl(): boolean { return this.#confirmed && this.#status.supported && this.#status.owned; }
  get inputGeneration(): string | undefined {
    return this.ownsControl ? this.#status.generation ?? undefined : undefined;
  }
  get observedGeneration(): string | undefined {
    return this.#confirmed && this.#status.supported ? this.#status.generation ?? undefined : undefined;
  }

  disconnect(): void { this.#confirmed = false; }

  observe(value: unknown): "accepted" | "stale" | "invalid" {
    const status = parseStatus(value);
    if (!status) {
      this.#confirmed = false;
      return "invalid";
    }
    if (status.supported) {
      const generation = BigInt(status.generation!);
      if (this.#generation !== undefined && (generation < this.#generation
        || (generation === this.#generation && this.#expired && status.occupied))) return "stale";
      if (generation === this.#generation && this.#status.supported && this.#status.occupied
        && status.occupied && this.#status.owned !== status.owned) {
        this.#confirmed = false;
        return "invalid";
      }
      if (generation !== this.#generation) this.#expired = false;
      this.#generation = generation;
      // Only an engine-reported empty owner expires a generation. A local
      // deadline or transport loss must not prevent same-view reconnect.
      this.#expired ||= !status.occupied;
    }
    this.#status = status;
    this.#confirmed = true;
    return "accepted";
  }
}

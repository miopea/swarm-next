/**
 * How long a lease has left, rounded the way a person reads it.
 *
 * Shared by both ends of a takeover so they cannot disagree about what "about
 * five minutes" means. Never negative: a lease already past its time reads 0s
 * rather than a countdown into the past.
 */
export function approximateRemaining(expiresAt: number, now = Date.now()): string {
  const seconds = Math.max(0, Math.round(expiresAt - now / 1000));
  return seconds >= 60 ? `${Math.ceil(seconds / 60)} min` : `${seconds}s`;
}

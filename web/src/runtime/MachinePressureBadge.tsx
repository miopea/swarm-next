import type { MachinePressureNotice } from "./machinePressure";

/**
 * The machine-pressure badge that sits beside the runtime version.
 *
 * THREE CHANNELS, NOT ONE. The operator asked for "the heartbeat thing
 * changing colour or something". Colour alone fails anyone who cannot rely on
 * it, and this app targets WCAG 2.1 AA, so each level carries a distinct SHAPE
 * and its own TEXT as well as its own hue. Remove the stylesheet entirely and
 * the badge still reads correctly; view it in greyscale and the outline still
 * separates the three states.
 *
 * The shapes are chosen to be distinguishable at 12px and without colour:
 * a warning triangle for advisory, a filled octagon for critical, a dashed
 * circle for unknown. An unknown that borrowed the triangle would read as a
 * mild warning, which is exactly the wrong thing to say about a reading nobody
 * has.
 */
function LevelGlyph({ level }: { level: MachinePressureNotice["level"] }) {
  if (level === "critical") {
    return (
      <svg className="machine-pressure-glyph" viewBox="0 0 24 24" aria-hidden="true">
        <path d="M8 2h8l6 6v8l-6 6H8l-6-6V8Z" fill="currentColor" stroke="none" />
        <path d="M12 7v6" stroke="var(--surface)" strokeWidth="2.5" strokeLinecap="round" />
        <circle cx="12" cy="17" r="1.4" fill="var(--surface)" stroke="none" />
      </svg>
    );
  }
  if (level === "advisory") {
    return (
      <svg className="machine-pressure-glyph" viewBox="0 0 24 24" aria-hidden="true">
        <path d="M12 3 22 20H2Z" fill="none" strokeWidth="2" strokeLinejoin="round" />
        <path d="M12 9v5" strokeWidth="2" strokeLinecap="round" />
        <circle cx="12" cy="17.4" r="1.2" fill="currentColor" stroke="none" />
      </svg>
    );
  }
  return (
    <svg className="machine-pressure-glyph" viewBox="0 0 24 24" aria-hidden="true">
      <circle cx="12" cy="12" r="9" fill="none" strokeWidth="2" strokeDasharray="3 3" />
      <path d="M8.5 12h7" strokeWidth="2" strokeLinecap="round" />
    </svg>
  );
}

/**
 * Renders nothing when there is nothing to say — a quiet header is what makes
 * a badge worth reading. `role="status"` announces the change to a screen
 * reader without stealing focus, which suits a condition the operator should
 * notice rather than be interrupted by.
 */
export default function MachinePressureBadge({ notice }: { notice: MachinePressureNotice | null }) {
  if (!notice) return null;
  return (
    <span
      className={`machine-pressure ${notice.level}`}
      role="status"
      title={`${notice.label}. ${notice.detail}`}
    >
      <LevelGlyph level={notice.level} />
      <span className="machine-pressure-label">{notice.label}</span>
      <span className="visually-hidden">. {notice.detail}</span>
    </span>
  );
}

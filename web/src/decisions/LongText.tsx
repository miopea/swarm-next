import { useState } from "react";

/** Beyond this many characters a block is folded until asked for. */
const FOLD_ABOVE = 700;

/**
 * Prose written by a worker, shown the way they wrote it.
 *
 * These blocks arrive with paragraphs and headings in them — one observed
 * request carried sixteen line breaks in its reason and twenty-three in its
 * evidence — and rendering them as a single run of text threw all of that away
 * and produced a wall nobody can scan.
 *
 * Long blocks are folded rather than truncated. The operator is deciding
 * something; nothing that was written for them is thrown away, but the queue
 * stays readable while they choose which one to read.
 */
export default function LongText({ text, label, foldAbove = FOLD_ABOVE }: { text: string; label: string; foldAbove?: number }) {
  const [expanded, setExpanded] = useState(false);
  const folds = text.length > foldAbove;
  if (!folds) return <p className="decision-prose">{text}</p>;
  return (
    <div className="decision-prose-folded">
      <p className={expanded ? "decision-prose" : "decision-prose clamped"}>{text}</p>
      <button type="button" className="text-button" onClick={() => setExpanded((open) => !open)}>
        {expanded ? `Show less of ${label}` : `Show all of ${label}`}
      </button>
    </div>
  );
}

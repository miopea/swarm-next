import { useState } from "react";
import { createPortal } from "react-dom";

import { useModalFocus } from "./useModalFocus";

/**
 * An attached image, at a size a person can actually read.
 *
 * The thumbnail is a button rather than a picture. That is the point of this
 * component, not a detail of it: an image nobody can read does not present as
 * inconvenient, it presents as ABSENT, and people then act on its absence.
 * Queen read two email-sourced tasks as having lost their screenshots and wrote
 * briefs on that premise — telling a worker to interview the operator instead
 * of opening the code, and putting one of them to the operator as a decision.
 * The bytes were in the attachment store the whole time. So where bytes exist,
 * the affordance says so.
 *
 * Viewing, not downloading. Several surfaces already hand the file over; the
 * ask here was to look at it without leaving the page.
 */
export default function ImageViewer({ src, filename, caption }: {
  src: string;
  filename: string;
  /** Shown under the thumbnail. Defaults to the filename. */
  caption?: string;
}) {
  const [open, setOpen] = useState(false);
  return (
    <figure className="image-viewer">
      <button
        type="button"
        className="image-viewer-trigger"
        aria-label={`View ${filename} at full size`}
        onClick={() => setOpen(true)}
      >
        <img src={src} alt={filename} />
        <span className="image-viewer-hint" aria-hidden="true">Zoom</span>
      </button>
      <figcaption>{caption ?? filename}</figcaption>
      {open && <ImageViewerOverlay src={src} filename={filename} onClose={() => setOpen(false)} />}
    </figure>
  );
}

function ImageViewerOverlay({ src, filename, onClose }: {
  src: string;
  filename: string;
  onClose: () => void;
}) {
  // Escape, focus containment and focus restoration all come from the shared
  // modal contract rather than being reimplemented here.
  const dialog = useModalFocus<HTMLDivElement>(onClose);
  // Portalled to the body so an oversized image cannot disturb the panel it was
  // opened from — the email panel is narrow and shares the screen with the task
  // form beside it.
  return createPortal(
    <div className="image-viewer-backdrop" onClick={onClose}>
      <div
        ref={dialog}
        className="image-viewer-dialog"
        role="dialog"
        aria-modal="true"
        aria-label={filename}
        tabIndex={-1}
        onClick={(event) => event.stopPropagation()}
      >
        <img src={src} alt={filename} />
        <div className="image-viewer-bar">
          <span title={filename}>{filename}</span>
          <button type="button" className="secondary-button" onClick={onClose}>Close</button>
        </div>
      </div>
    </div>,
    document.body,
  );
}

import { type PointerEvent as ReactPointerEvent, type ReactNode, useRef, useState } from "react";

interface ShellModalProps {
  title: string;
  subtitle: string;
  onClose: () => void;
  children: ReactNode;
}

/** Where the operator has dragged the window to, once they have dragged it. */
interface Placement {
  left: number;
  top: number;
}

/** How much of the header must stay on screen, so a window is always reachable. */
const KEEP_VISIBLE = 64;

/**
 * A draggable, resizable window holding a terminal.
 *
 * NO FOCUS TRAP, deliberately, and this is the constraint the whole component is
 * shaped around. The shared useModalFocus hook intercepts Escape and Tab to move
 * between controls — and both are load-bearing keystrokes in a shell: Escape
 * cancels, Tab completes a path. A dialog that swallowed them would be a worse
 * terminal than no dialog. Closing is the button or the backdrop.
 *
 * Resizing is the browser's own `resize: both` rather than hand-written handles,
 * because the terminal inside already watches its container with a
 * ResizeObserver — so the pty follows the window with no wiring of ours.
 */
export default function ShellModal({ title, subtitle, onClose, children }: ShellModalProps) {
  const dialog = useRef<HTMLDivElement>(null);
  const drag = useRef<{ pointerX: number; pointerY: number; left: number; top: number }>(undefined);
  const [placement, setPlacement] = useState<Placement>();

  function startDrag(event: ReactPointerEvent<HTMLDivElement>) {
    // Primary button only, and never a press that starts on the close button.
    if (event.button !== 0 || !dialog.current) return;
    if ((event.target as HTMLElement).closest("button")) return;
    const box = dialog.current.getBoundingClientRect();
    drag.current = { pointerX: event.clientX, pointerY: event.clientY, left: box.left, top: box.top };
    event.currentTarget.setPointerCapture(event.pointerId);
  }

  function continueDrag(event: ReactPointerEvent<HTMLDivElement>) {
    const start = drag.current;
    if (!start || !dialog.current) return;
    const box = dialog.current.getBoundingClientRect();
    // Clamped so the header can never be dragged off screen: a window the
    // operator cannot reach is a window they cannot close.
    setPlacement({
      left: Math.min(
        Math.max(start.left + event.clientX - start.pointerX, KEEP_VISIBLE - box.width),
        window.innerWidth - KEEP_VISIBLE,
      ),
      top: Math.min(Math.max(start.top + event.clientY - start.pointerY, 0), window.innerHeight - KEEP_VISIBLE),
    });
  }

  function endDrag(event: ReactPointerEvent<HTMLDivElement>) {
    drag.current = undefined;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
  }

  return (
    <div
      className="shell-modal-backdrop"
      role="presentation"
      onMouseDown={(event) => { if (event.target === event.currentTarget) onClose(); }}
    >
      <div
        ref={dialog}
        className="shell-modal"
        role="dialog"
        aria-modal="true"
        aria-label={`${title}: ${subtitle}`}
        style={placement ? { position: "fixed", left: placement.left, top: placement.top, margin: 0 } : undefined}
      >
        <div
          className="shell-modal-header"
          onPointerDown={startDrag}
          onPointerMove={continueDrag}
          onPointerUp={endDrag}
          onPointerCancel={endDrag}
        >
          <strong>{title}</strong>
          <small>{subtitle}</small>
          <button type="button" className="shell-modal-close" onClick={onClose}>Close</button>
        </div>
        {children}
      </div>
    </div>
  );
}

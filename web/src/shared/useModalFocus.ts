import { useEffect, useRef, type RefObject } from "react";

const focusableSelector = [
  "a[href]",
  "button:not([disabled])",
  "input:not([disabled]):not([type='hidden'])",
  "select:not([disabled])",
  "textarea:not([disabled])",
  "[tabindex]:not([tabindex='-1'])",
].join(",");

/** Gives every modal the same keyboard contract: enter, contain, escape, restore. */
export function useModalFocus<T extends HTMLElement>(
  onClose: () => void,
  enabled = true,
  initialFocus?: RefObject<HTMLElement | null>,
) {
  const dialog = useRef<T>(null);
  const close = useRef(onClose);
  close.current = onClose;

  useEffect(() => {
    if (!enabled || !dialog.current) return;
    const previouslyFocused = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const modal = dialog.current;
    const focusable = () => [...modal.querySelectorAll<HTMLElement>(focusableSelector)]
      .filter((element) => !element.hasAttribute("disabled") && element.getAttribute("aria-hidden") !== "true");
    (initialFocus?.current ?? focusable()[0] ?? modal).focus();

    function keyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        event.preventDefault();
        close.current();
        return;
      }
      if (event.key !== "Tab") return;
      const items = focusable();
      if (!items.length) {
        event.preventDefault();
        modal.focus();
        return;
      }
      const first = items[0];
      const last = items[items.length - 1];
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    }

    window.addEventListener("keydown", keyDown);
    return () => {
      window.removeEventListener("keydown", keyDown);
      if (previouslyFocused?.isConnected) previouslyFocused.focus();
    };
  }, [enabled, initialFocus]);

  return dialog;
}

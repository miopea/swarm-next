import { useEffect, useRef, type RefObject } from "react";

const focusableSelector = [
  "a[href]",
  "button:not([disabled])",
  "input:not([disabled]):not([type='hidden'])",
  "select:not([disabled])",
  "textarea:not([disabled])",
  "summary",
  "[tabindex]:not([tabindex='-1'])",
].join(",");

const modalSelector = "[data-swarm-modal-focus]";
export function hasActiveModal(): boolean {
  return document.querySelector(modalSelector) !== null;
}
// The mounted DOM owns modal order, including portals. No retained stack or timer.
function topModal(): HTMLElement | undefined {
  return [...document.querySelectorAll<HTMLElement>(modalSelector)].at(-1);
}

function visible(element: HTMLElement, modal: HTMLElement): boolean {
  for (let node: HTMLElement | null = element; node; node = node.parentElement) {
    if (node.hidden || node.hasAttribute("inert") || node.getAttribute("aria-hidden") === "true") return false;
    const style = getComputedStyle(node);
    if (style.display === "none" || style.visibility === "hidden" || style.visibility === "collapse") return false;
    if (node instanceof HTMLDetailsElement && !node.open) {
      const summary = [...node.children].find(child => child.tagName === "SUMMARY");
      if (!summary?.contains(element)) return false;
    }
    if (node === modal) break;
  }
  return true;
}

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
    modal.setAttribute("data-swarm-modal-focus", "");
    const focusable = () => [...modal.querySelectorAll<HTMLElement>(focusableSelector)]
      .filter((element) => !element.hasAttribute("disabled") && visible(element, modal));
    let lastFocused: HTMLElement | null = null;
    function focusInside() {
      const preferred = lastFocused ?? initialFocus?.current;
      (preferred?.isConnected && modal.contains(preferred) && visible(preferred, modal)
        ? preferred : focusable()[0] ?? modal).focus();
    }
    function focusIn(event: FocusEvent) {
      if (topModal() !== modal) return;
      if (event.target instanceof HTMLElement && modal.contains(event.target)) lastFocused = event.target;
      else focusInside();
    }
    document.addEventListener("focusin", focusIn);
    if (topModal() === modal) focusInside();

    function keyDown(event: KeyboardEvent) {
      if (topModal() !== modal || event.defaultPrevented) return;
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopImmediatePropagation();
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
      if (event.shiftKey && (document.activeElement === first || document.activeElement === modal)) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && (document.activeElement === last || document.activeElement === modal)) {
        event.preventDefault();
        first.focus();
      }
    }

    window.addEventListener("keydown", keyDown);
    return () => {
      window.removeEventListener("keydown", keyDown);
      document.removeEventListener("focusin", focusIn);
      modal.removeAttribute("data-swarm-modal-focus");
      const remaining = topModal();
      if (previouslyFocused?.isConnected && (!remaining || remaining.contains(previouslyFocused))) previouslyFocused.focus();
    };
  }, [enabled, initialFocus]);

  return dialog;
}

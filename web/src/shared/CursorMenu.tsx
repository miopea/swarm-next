import { useLayoutEffect, useRef, useState, type KeyboardEvent as ReactKeyboardEvent, type ReactNode } from "react";
import { createPortal } from "react-dom";

export type MenuPoint = { x: number; y: number };

type Props = {
  label: string;
  point: MenuPoint;
  onClose: () => void;
  children: ReactNode;
  className?: string;
};

const VIEWPORT_GUTTER = 8;

export function pointFromElement(element: HTMLElement): MenuPoint {
  const bounds = element.getBoundingClientRect();
  return { x: bounds.right, y: bounds.bottom };
}

export default function CursorMenu({ label, point, onClose, children, className = "" }: Props) {
  const menuRef = useRef<HTMLDivElement>(null);
  const [position, setPosition] = useState(point);

  useLayoutEffect(() => {
    const menu = menuRef.current;
    if (!menu) return;
    const bounds = menu.getBoundingClientRect();
    setPosition({
      x: Math.max(VIEWPORT_GUTTER, Math.min(point.x, window.innerWidth - bounds.width - VIEWPORT_GUTTER)),
      y: Math.max(VIEWPORT_GUTTER, Math.min(point.y, window.innerHeight - bounds.height - VIEWPORT_GUTTER)),
    });
    const firstAction = menu.querySelector<HTMLElement>('[role="menuitem"]:not([disabled])');
    (firstAction ?? menu).focus();
  }, [point]);

  function moveFocus(event: ReactKeyboardEvent<HTMLDivElement>) {
    const actions = Array.from(menuRef.current?.querySelectorAll<HTMLElement>('[role="menuitem"]:not([disabled])') ?? []);
    if (!actions.length) return;
    const current = actions.indexOf(document.activeElement as HTMLElement);
    let target: number | undefined;
    if (event.key === "ArrowDown") target = current < 0 || current === actions.length - 1 ? 0 : current + 1;
    if (event.key === "ArrowUp") target = current <= 0 ? actions.length - 1 : current - 1;
    if (event.key === "Home") target = 0;
    if (event.key === "End") target = actions.length - 1;
    if (target === undefined) return;
    event.preventDefault();
    actions[target].focus();
  }

  useLayoutEffect(() => {
    function dismiss(event: PointerEvent) {
      if (event.target instanceof Node && !menuRef.current?.contains(event.target)) onClose();
    }
    function closeOnEscape(event: KeyboardEvent) {
      if (event.key === "Escape") onClose();
    }
    function closeOnViewportChange() { onClose(); }
    document.addEventListener("pointerdown", dismiss);
    document.addEventListener("keydown", closeOnEscape);
    window.addEventListener("resize", closeOnViewportChange);
    window.addEventListener("scroll", closeOnViewportChange, true);
    return () => {
      document.removeEventListener("pointerdown", dismiss);
      document.removeEventListener("keydown", closeOnEscape);
      window.removeEventListener("resize", closeOnViewportChange);
      window.removeEventListener("scroll", closeOnViewportChange, true);
    };
  }, [onClose]);

  return createPortal(
    <div
      ref={menuRef}
      className={`cursor-menu ${className}`.trim()}
      role="menu"
      aria-label={label}
      tabIndex={-1}
      style={{ left: position.x, top: position.y }}
      onKeyDown={moveFocus}
    >
      {children}
    </div>,
    document.body,
  );
}

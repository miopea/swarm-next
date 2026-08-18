import { useLayoutEffect, useRef, useState, type ReactNode } from "react";
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
    menu.focus();
  }, [point]);

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
    >
      {children}
    </div>,
    document.body,
  );
}

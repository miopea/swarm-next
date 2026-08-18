import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { afterEach, describe, expect, test, vi } from "vitest";

import CursorMenu, { type MenuPoint } from "./CursorMenu";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

function Harness({ initial }: { initial: MenuPoint }) {
  const [point, setPoint] = useState<MenuPoint | undefined>(initial);
  return point ? <CursorMenu label="Actions" point={point} onClose={() => setPoint(undefined)}><button role="menuitem">Open</button><button role="menuitem" disabled>Unavailable</button><button role="menuitem">Remove</button></CursorMenu> : null;
}

describe("CursorMenu", () => {
  test("opens at the pointer when it fits and closes on outside input", () => {
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue({ width: 180, height: 120, x: 0, y: 0, top: 0, left: 0, right: 180, bottom: 120, toJSON: () => ({}) });
    render(<Harness initial={{ x: 240, y: 180 }} />);
    const menu = screen.getByRole("menu", { name: "Actions" });
    expect(menu).toHaveStyle({ left: "240px", top: "180px" });
    fireEvent.pointerDown(document.body);
    expect(screen.queryByRole("menu", { name: "Actions" })).not.toBeInTheDocument();
  });

  test("flips inside the viewport at the bottom-right edge", () => {
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue({ width: 180, height: 120, x: 0, y: 0, top: 0, left: 0, right: 180, bottom: 120, toJSON: () => ({}) });
    Object.defineProperty(window, "innerWidth", { configurable: true, value: 500 });
    Object.defineProperty(window, "innerHeight", { configurable: true, value: 400 });
    render(<Harness initial={{ x: 490, y: 390 }} />);
    expect(screen.getByRole("menu", { name: "Actions" })).toHaveStyle({ left: "312px", top: "272px" });
  });

  test("focuses an available action and supports standard menu navigation", () => {
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue({ width: 180, height: 120, x: 0, y: 0, top: 0, left: 0, right: 180, bottom: 120, toJSON: () => ({}) });
    render(<Harness initial={{ x: 40, y: 40 }} />);

    const open = screen.getByRole("menuitem", { name: "Open" });
    const remove = screen.getByRole("menuitem", { name: "Remove" });
    expect(open).toHaveFocus();
    fireEvent.keyDown(open, { key: "ArrowDown" });
    expect(remove).toHaveFocus();
    fireEvent.keyDown(remove, { key: "ArrowDown" });
    expect(open).toHaveFocus();
    fireEvent.keyDown(open, { key: "End" });
    expect(remove).toHaveFocus();
    fireEvent.keyDown(remove, { key: "Home" });
    expect(open).toHaveFocus();
  });
});

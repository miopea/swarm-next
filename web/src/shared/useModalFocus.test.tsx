import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useRef, useState } from "react";
import { afterEach, expect, test } from "vitest";

import { useModalFocus } from "./useModalFocus";

afterEach(cleanup);

function Modal({ onClose }: { onClose: () => void }) {
  const first = useRef<HTMLInputElement>(null);
  const dialog = useModalFocus<HTMLElement>(onClose, true, first);
  return <section ref={dialog} tabIndex={-1} role="dialog" aria-label="Test modal">
    <input ref={first} aria-label="First field" />
    <button type="button" onClick={onClose}>Close modal</button>
  </section>;
}

function Harness() {
  const [open, setOpen] = useState(false);
  return <><button type="button" onClick={() => setOpen(true)}>Open modal</button>{open ? <Modal onClose={() => setOpen(false)} /> : null}</>;
}

test("focuses, contains, closes, and returns keyboard focus for a modal", async () => {
  render(<Harness />);
  const trigger = screen.getByRole("button", { name: "Open modal" });
  trigger.focus();
  fireEvent.click(trigger);

  const first = screen.getByLabelText("First field");
  await waitFor(() => expect(first).toHaveFocus());

  const close = screen.getByRole("button", { name: "Close modal" });
  close.focus();
  fireEvent.keyDown(document, { key: "Tab" });
  expect(first).toHaveFocus();

  fireEvent.keyDown(document, { key: "Escape" });
  expect(screen.queryByRole("dialog", { name: "Test modal" })).not.toBeInTheDocument();
  expect(trigger).toHaveFocus();
});

function NestedModal() {
  const [outer, setOuter] = useState(true);
  const [inner, setInner] = useState(false);
  const dialog = useModalFocus<HTMLElement>(() => setOuter(false), outer);
  return <><input aria-label="Outside terminal stand-in" />{outer && <section ref={dialog} tabIndex={-1} role="dialog" aria-label="Outer">
    <button onClick={() => setInner(true)}>Open inner</button>
    {inner && <Modal onClose={() => setInner(false)} />}
  </section>}</>;
}

test("Escape closes only the top dialog and restores its trigger", () => {
  render(<NestedModal />);
  const trigger = screen.getByRole("button", { name: "Open inner" });
  fireEvent.click(trigger);
  fireEvent.keyDown(document, { key: "Escape" });
  expect(screen.queryByRole("dialog", { name: "Test modal" })).not.toBeInTheDocument();
  expect(screen.getByRole("dialog", { name: "Outer" })).toBeInTheDocument();
  expect(trigger).toHaveFocus();
});

test("a background component cannot move keyboard focus outside the active modal", () => {
  render(<NestedModal />);
  fireEvent.click(screen.getByRole("button", { name: "Open inner" }));
  screen.getByLabelText("Outside terminal stand-in").focus();
  expect(screen.getByLabelText("First field")).toHaveFocus();
});

function HiddenModal() {
  const dialog = useModalFocus<HTMLElement>(() => undefined);
  return <section ref={dialog} tabIndex={-1} role="dialog">
    <div hidden><button>Hidden action</button></div>
    <details><summary>More actions</summary><button>Collapsed action</button></details>
    <button>Visible action</button>
  </section>;
}

test("hidden and collapsed controls do not become focus-trap endpoints", () => {
  render(<HiddenModal />);
  const visible = screen.getByRole("button", { name: "Visible action" });
  visible.focus();
  fireEvent.keyDown(document, { key: "Tab" });
  expect(screen.getByText("More actions")).toHaveFocus();
});

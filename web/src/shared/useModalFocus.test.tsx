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

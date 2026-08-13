import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

import {
  composeTerminalSubmission,
  MAX_TERMINAL_DRAFT_LENGTH,
  MOBILE_TERMINAL_KEYS,
  MobileTerminalComposer,
} from "./MobileTerminalComposer";

afterEach(cleanup);
beforeEach(() => localStorage.clear());

test("sends slash commands through the terminal unchanged and appends Enter", () => {
  const onInput = vi.fn();
  render(<MobileTerminalComposer connectionState="connected" onInput={onInput} />);

  fireEvent.change(screen.getByLabelText(/Message worker/), { target: { value: "/status" } });
  fireEvent.click(screen.getByRole("button", { name: "Send" }));

  expect(onInput).toHaveBeenCalledWith("/status\r");
  expect(screen.getByLabelText(/Message worker/)).toHaveValue("");
});

test("uses bracketed paste for multiline dictation before submitting once", () => {
  expect(composeTerminalSubmission("first\r\nsecond")).toBe(
    "\u001b[200~first\nsecond\u001b[201~\r",
  );
});

test("bounds drafts kept in the browser view", () => {
  render(<MobileTerminalComposer connectionState="connected" onInput={vi.fn()} />);
  const input = screen.getByLabelText(/Message worker/);

  fireEvent.change(input, { target: { value: "x".repeat(MAX_TERMINAL_DRAFT_LENGTH + 100) } });

  expect(input).toHaveValue("x".repeat(MAX_TERMINAL_DRAFT_LENGTH));
});

test("sends mobile navigation and Claude mode controls as terminal key sequences", () => {
  const onInput = vi.fn();
  render(<MobileTerminalComposer connectionState="connected" onInput={onInput} />);

  fireEvent.click(screen.getByRole("button", { name: "Arrow up" }));
  fireEvent.click(screen.getByRole("button", { name: "Arrow left" }));
  fireEvent.click(screen.getByRole("button", { name: "Arrow down" }));
  fireEvent.click(screen.getByRole("button", { name: "Arrow right" }));
  fireEvent.click(screen.getByRole("button", { name: "Enter" }));
  fireEvent.click(screen.getByRole("button", { name: "Esc" }));
  fireEvent.click(screen.getByRole("button", { name: "Tab" }));
  fireEvent.click(screen.getByRole("button", { name: "Ctrl+C" }));
  fireEvent.click(screen.getByRole("button", { name: "Cycle mode" }));

  expect(onInput.mock.calls.map(([value]) => value)).toEqual([
    MOBILE_TERMINAL_KEYS.up,
    MOBILE_TERMINAL_KEYS.left,
    MOBILE_TERMINAL_KEYS.down,
    MOBILE_TERMINAL_KEYS.right,
    MOBILE_TERMINAL_KEYS.enter,
    MOBILE_TERMINAL_KEYS.escape,
    MOBILE_TERMINAL_KEYS.tab,
    MOBILE_TERMINAL_KEYS.interrupt,
    MOBILE_TERMINAL_KEYS.modeCycle,
  ]);
});

test("remembers when the operator collapses the mobile key pad", () => {
  const first = render(<MobileTerminalComposer connectionState="connected" onInput={vi.fn()} />);
  fireEvent.click(screen.getByRole("button", { name: "Hide keys" }));
  expect(screen.queryByRole("button", { name: "Arrow up" })).not.toBeInTheDocument();
  first.unmount();

  render(<MobileTerminalComposer connectionState="connected" onInput={vi.fn()} />);
  expect(screen.getByRole("button", { name: "Show keys" })).toHaveAttribute("aria-expanded", "false");
});

test("reports controlled key visibility for the durable mobile profile", () => {
  const onKeysExpandedChange = vi.fn();
  render(
    <MobileTerminalComposer
      connectionState="connected"
      keysExpanded={false}
      onInput={vi.fn()}
      onKeysExpandedChange={onKeysExpandedChange}
    />,
  );

  fireEvent.click(screen.getByRole("button", { name: "Show keys" }));

  expect(onKeysExpandedChange).toHaveBeenCalledWith(true);
});

test("retains the draft and blocks controls while disconnected", () => {
  const onInput = vi.fn();
  render(<MobileTerminalComposer connectionState="disconnected" onInput={onInput} />);

  fireEvent.change(screen.getByLabelText(/Message worker/), { target: { value: "keep me" } });
  fireEvent.click(screen.getByRole("button", { name: "Send" }));

  expect(onInput).not.toHaveBeenCalled();
  expect(screen.getByLabelText(/Message worker/)).toHaveValue("keep me");
  expect(screen.getByRole("button", { name: "Cycle mode" })).toBeDisabled();
  expect(screen.getByRole("button", { name: "Add image" })).toBeDisabled();
});

test("offers a first-class mobile image picker without submitting the draft", async () => {
  const onImage = vi.fn().mockResolvedValue(undefined);
  const { container } = render(<MobileTerminalComposer connectionState="connected" onInput={vi.fn()} onImage={onImage} />);
  const image = new File([new Uint8Array([1, 2, 3])], "screen.png", { type: "image/png" });
  const input = container.querySelector<HTMLInputElement>('input[type="file"]');

  fireEvent.change(input!, { target: { files: [image] } });

  await vi.waitFor(() => expect(onImage).toHaveBeenCalledWith(image));
  expect(screen.getByRole("button", { name: "Add image" })).toBeEnabled();
});

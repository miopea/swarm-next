import { fireEvent, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";

import WhatsNewModal from "./WhatsNewModal";

const release = (version: string, summary: string, engine = false) => ({
  version,
  notes: [{ summary, kind: "feature", needs_worker_engine_update: engine }],
});

test("nothing renders when there is nothing new", () => {
  const { container } = render(<WhatsNewModal releases={[]} onDismiss={() => {}} />);
  expect(container).toBeEmptyDOMElement();
});

test("every skipped release is listed, not only the newest", () => {
  render(
    <WhatsNewModal
      releases={[release("0.8.19", "A shell opens from the menu"), release("0.8.17", "Attachments stop being rejected")]}
      onDismiss={() => {}}
    />,
  );
  expect(screen.getByText("A shell opens from the menu")).toBeInTheDocument();
  expect(screen.getByText("Attachments stop being rejected")).toBeInTheDocument();
});

/**
 * The caveat Queen called the sharpest part of the brief: a host-side change is
 * installed and not in effect, and announcing it as available is a confident
 * false claim about what the operator can do right now.
 */
test("a host-side change says it is not in effect yet", () => {
  render(<WhatsNewModal releases={[release("0.8.19", "Workers get a new engine trick", true)]} onDismiss={() => {}} />);
  expect(screen.getByText(/after the worker engine update/)).toBeInTheDocument();
  expect(screen.getByText(/installed but not running yet/)).toBeInTheDocument();
});

test("a change that needs no engine update carries no such warning", () => {
  render(<WhatsNewModal releases={[release("0.8.19", "A button moved", false)]} onDismiss={() => {}} />);
  expect(screen.queryByText(/after the worker engine update/)).toBeNull();
  expect(screen.queryByText(/installed but not running yet/)).toBeNull();
});

test("dismissing reports it once", () => {
  const onDismiss = vi.fn();
  render(<WhatsNewModal releases={[release("0.8.19", "A shell opens")]} onDismiss={onDismiss} />);
  fireEvent.click(screen.getByRole("button", { name: "Got it" }));
  expect(onDismiss).toHaveBeenCalledTimes(1);
});

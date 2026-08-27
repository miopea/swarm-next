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

/**
 * Grouped, not interleaved. Someone scanning for what they can now DO should
 * not have to read past a list of repairs to find it.
 */
test("separates new features from fixes", () => {
  render(
    <WhatsNewModal
      releases={[{
        version: "0.8.18",
        notes: [
          { summary: "a shell opens from the menu", kind: "feature", needs_worker_engine_update: false },
          { summary: "the shell window stops jumping", kind: "fix", needs_worker_engine_update: false },
        ],
      }]}
      onDismiss={() => {}}
    />,
  );
  expect(screen.getByText("New features")).toBeInTheDocument();
  expect(screen.getByText("Fixes")).toBeInTheDocument();
});

/** A release with only repairs does not show an empty features heading. */
test("omits a section that has nothing in it", () => {
  render(
    <WhatsNewModal
      releases={[{
        version: "0.8.18",
        notes: [{ summary: "the box stops failing", kind: "fix", needs_worker_engine_update: false }],
      }]}
      onDismiss={() => {}}
    />,
  );
  expect(screen.queryByText("New features")).toBeNull();
  expect(screen.getByText("Fixes")).toBeInTheDocument();
});

/** Commit subjects are written lowercase to follow a verb; the modal is not a git log. */
test("reads each line as a sentence", () => {
  render(
    <WhatsNewModal
      releases={[{
        version: "0.8.18",
        notes: [{ summary: "a shell opens from the menu", kind: "feature", needs_worker_engine_update: false }],
      }]}
      onDismiss={() => {}}
    />,
  );
  expect(screen.getByText("A shell opens from the menu")).toBeInTheDocument();
});

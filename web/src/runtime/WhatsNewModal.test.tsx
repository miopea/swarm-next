import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, test, vi } from "vitest";

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

describe("earlier releases", () => {
  const release = (version: string, summary: string) => ({
    version,
    notes: [{ summary, kind: "fix" as const, needs_worker_engine_update: false }],
  });

  it("does not show the history until it is asked for", () => {
    render(
      <WhatsNewModal
        releases={[release("1.0.1", "the panel is wider")]}
        earlier={[release("1.0.0", "feedback reaches GitHub"), release("0.9.2", "an update confirms the stop")]}
        onDismiss={() => {}}
      />,
    );
    // The reason this is not a changelog still holds: someone who just updated
    // wants what changed, not everything that ever changed.
    expect(screen.queryByText(/Feedback reaches GitHub/)).toBeNull();
    expect(screen.getByRole("button", { name: /Earlier releases \(2\)/ })).toBeTruthy();
  });

  it("opens the history in the same panel", async () => {
    render(
      <WhatsNewModal
        releases={[release("1.0.1", "the panel is wider")]}
        earlier={[release("1.0.0", "feedback reaches GitHub"), release("0.9.2", "an update confirms the stop")]}
        onDismiss={() => {}}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /Earlier releases/ }));
    expect(screen.getByText(/Feedback reaches GitHub/)).toBeTruthy();
    expect(screen.getByText(/An update confirms the stop/)).toBeTruthy();
    // Each earlier block is labelled, because out there the version is the only
    // thing telling one from the next.
    expect(screen.getByText("1.0.0")).toBeTruthy();
    expect(screen.getByText("0.9.2")).toBeTruthy();
  });

  it("offers nothing to open when the artifact carries no older releases", () => {
    render(<WhatsNewModal releases={[release("1.0.1", "the panel is wider")]} earlier={[]} onDismiss={() => {}} />);
    expect(screen.queryByRole("button", { name: /Earlier releases/ })).toBeNull();
  });

  it("reads as a deliberate visit when opened from settings", () => {
    render(
      <WhatsNewModal
        releases={[release("1.0.1", "the panel is wider")]}
        heading="Release notes"
        onDismiss={() => {}}
      />,
    );
    expect(screen.getByRole("heading", { name: "Release notes" })).toBeTruthy();
    // "Got it" is an acknowledgement of news; this was not news.
    expect(screen.getByRole("button", { name: "Close" })).toBeTruthy();
  });
});

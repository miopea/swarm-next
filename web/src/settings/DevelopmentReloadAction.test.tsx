import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import DevelopmentReloadAction from "./DevelopmentReloadAction";

afterEach(cleanup);

test("explains a failed build without implying that workers or the current app stopped", () => {
  const reload = vi.fn().mockResolvedValue(undefined);
  render(<DevelopmentReloadAction busy={false} onReload={reload} runtime={{
    enabled: true,
    version: "0.1.0-dev-123456789abc-20260815040000-10",
    state: "failed",
    reload_available: true,
    deployed_source_revision: "76543210fedc",
    source_revision: "abcdef012345",
    source_dirty: false,
  }} />);

  const status = screen.getByLabelText("App and API status");
  expect(status).toHaveTextContent("Development build failed");
  expect(status).toHaveTextContent("Revision 7654321 is still serving this page");
  expect(status).toHaveTextContent("Workers were never restarted or interrupted");
  fireEvent.click(screen.getByRole("button", { name: "Retry development build" }));
  fireEvent.click(screen.getByRole("button", { name: "Build and reload" }));
  expect(reload).toHaveBeenCalledOnce();
});

test("names both revisions while a safe app reload is available", () => {
  render(<DevelopmentReloadAction busy={false} onReload={vi.fn()} runtime={{
    enabled: true,
    version: "0.1.0-dev-123456789abc-20260815040000-10",
    state: "idle",
    reload_available: true,
    deployed_source_revision: "76543210fedc",
    source_revision: "abcdef012345",
    source_dirty: false,
  }} />);

  expect(screen.getByLabelText("App and API status")).toHaveTextContent(
    "Revision 7654321 is active. Build and switch the browser and API to working-copy revision abcdef0.",
  );
});

test("explains that the running build matches the polled working copy", () => {
  render(<DevelopmentReloadAction busy={false} onReload={vi.fn()} runtime={{
    enabled: true,
    version: "0.1.0-dev-123456789abc-20260815040000-10",
    state: "ready",
    reload_available: false,
    deployed_source_revision: "abcdef012345",
    source_revision: "abcdef012345",
    source_dirty: false,
  }} />);

  const status = screen.getByLabelText("App and API status");
  expect(status).toHaveTextContent("Running build matches the working copy");
  expect(status).toHaveTextContent("Active revision abcdef0 matches the product code in this checkout");
  expect(status).toHaveTextContent("Swarm checks the working copy every 15 seconds");
  expect(status).toHaveTextContent("without restarting Claude, Codex, or the worker engine");
});

test("blocks an older or unrelated development checkout", () => {
  render(<DevelopmentReloadAction busy={false} onReload={vi.fn()} runtime={{
    enabled: true,
    version: "0.1.0-123456789abc",
    state: "source_mismatch",
    reload_available: false,
    deployed_source_revision: "76543210fedc",
    source_revision: "abcdef012345",
    source_dirty: false,
  }} />);

  const status = screen.getByLabelText("App and API status");
  expect(status).toHaveTextContent("Development checkout needs to catch up");
  expect(status).toHaveTextContent("Revision 7654321 is active");
  expect(status).toHaveTextContent("working-copy revision abcdef0 does not contain that deployed source");
  expect(screen.queryByRole("button", { name: /reload|build/i })).not.toBeInTheDocument();
});

test("shows a build in progress the way the worker engine card does", () => {
  // The build ran with nothing but a change of wording to show for it, so the
  // operator could not find it. It now carries the same live progress block as
  // the worker engine update, in the card where they started it.
  const { container } = render(<DevelopmentReloadAction busy={false} onReload={vi.fn()} runtime={{
    enabled: true,
    version: "0.1.0-dev-123456789abc-20260815040000-10",
    state: "building",
    reload_available: false,
    deployed_source_revision: "76543210fedc",
    source_revision: "abcdef012345",
    source_dirty: false,
  }} />);

  const status = screen.getByLabelText("App and API status");
  expect(status).toHaveTextContent("Building App and API…");
  expect(status).toHaveTextContent("Compiling and checking abcdef0");
  expect(status).toHaveTextContent("Revision 7654321 keeps serving this page");
  expect(container.querySelector(".maintenance-spinner")).not.toBeNull();
});

test("separates a build that has been asked for from one that is running", () => {
  render(<DevelopmentReloadAction busy={false} onReload={vi.fn()} runtime={{
    enabled: true,
    version: "0.1.0-dev-123456789abc-20260815040000-10",
    state: "requested",
    reload_available: false,
    deployed_source_revision: "76543210fedc",
    source_revision: "abcdef012345",
    source_dirty: false,
  }} />);

  expect(screen.getByLabelText("App and API status")).toHaveTextContent("Starting the App and API build…");
});

test("holds its place while the API restarts under the build", () => {
  // The operator watched a build compile and then the whole App and API card
  // disappeared, with a 502 alongside it. Vanishing mid-operation reads as the
  // build having destroyed something.
  render(<DevelopmentReloadAction busy={false} reachable={false} onReload={vi.fn()} runtime={{
    enabled: true,
    version: "0.1.0-dev-123456789abc-20260815040000-10",
    state: "building",
    reload_available: false,
    deployed_source_revision: "76543210fedc",
    source_revision: "abcdef012345",
    source_dirty: false,
  }} />);

  const status = screen.getByLabelText("App and API status");
  expect(status).toHaveTextContent("Reconnecting…");
  expect(status).toHaveTextContent("expected while a new build takes over");
});

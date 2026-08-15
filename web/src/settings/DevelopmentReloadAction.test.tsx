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
    source_revision: "abcdef012345",
    source_dirty: false,
  }} />);

  const status = screen.getByLabelText("App and API status");
  expect(status).toHaveTextContent("Development build failed");
  expect(status).toHaveTextContent("Revision 1234567 is still serving this page");
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
    source_revision: "abcdef012345",
    source_dirty: false,
  }} />);

  expect(screen.getByLabelText("App and API status")).toHaveTextContent(
    "Revision 1234567 is active. Build and switch the browser and API to working-copy revision abcdef0.",
  );
});

test("explains that the running build matches the polled working copy", () => {
  render(<DevelopmentReloadAction busy={false} onReload={vi.fn()} runtime={{
    enabled: true,
    version: "0.1.0-dev-123456789abc-20260815040000-10",
    state: "ready",
    reload_available: false,
    source_revision: "abcdef012345",
    source_dirty: false,
  }} />);

  const status = screen.getByLabelText("App and API status");
  expect(status).toHaveTextContent("Running build matches the working copy");
  expect(status).toHaveTextContent("Active revision 1234567 matches the product code in this checkout");
  expect(status).toHaveTextContent("Swarm checks the working copy every 15 seconds");
  expect(status).toHaveTextContent("without restarting Claude, Codex, or the worker engine");
});

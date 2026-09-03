import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import type { DevelopmentRuntime } from "../api";
import DeveloperDogfoodWorkspace from "./DeveloperDogfoodWorkspace";

afterEach(cleanup);
const runtime = { enabled: true, version: "dev-test", source_revision: "abc123", source_dirty: false } as DevelopmentRuntime;

test("uses development detection without another enable toggle", () => {
  const { rerender } = render(<DeveloperDogfoodWorkspace runtime={undefined} version="test" reachable={false} />);
  expect(screen.queryByText("Evidence from your daily Hive")).toBeNull();
  rerender(<DeveloperDogfoodWorkspace runtime={{ ...runtime, enabled: false }} version="test" reachable />);
  expect(screen.queryByText("Evidence from your daily Hive")).toBeNull();
  rerender(<DeveloperDogfoodWorkspace runtime={runtime} version="test" reachable />);
  expect(screen.getByText("Evidence from your daily Hive")).toBeTruthy();
  expect(screen.queryByRole("checkbox")).toBeNull();
  expect(screen.getByText(/Means and maxima are not percentiles/)).toBeTruthy();
});

test("qualifies unavailable status and only previews evidence on request", () => {
  const { container } = render(<DeveloperDogfoodWorkspace runtime={runtime} version="running-version" reachable={false} />);
  expect(screen.getByRole("status").textContent).toContain("last known");
  expect(container.querySelector("pre")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Refresh evidence" }));
  fireEvent.click(screen.getByRole("button", { name: "Preview browser evidence" }));
  const evidence = JSON.parse(container.querySelector("pre")!.textContent!);
  expect(evidence.running_version).toBe("running-version");
  expect(evidence.checkout_revision).toBe("abc123");
  expect(evidence.browser.current.schema).toBe(1);
  fireEvent.click(screen.getByRole("button", { name: "Preview browser evidence" }));
  expect(container.querySelector("pre")).toBeNull();
});

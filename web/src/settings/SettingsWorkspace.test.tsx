import { fireEvent, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";

import SettingsWorkspace from "./SettingsWorkspace";

test("shows runtime diagnostics and changes the selected theme", () => {
  const onThemeChange = vi.fn();
  render(<SettingsWorkspace colorTheme="light" health={{ status: "ok", version: "0.1.0" }} runningWorkers={2} retainedSessions={3} onThemeChange={onThemeChange} />);

  expect(screen.getByText("Healthy · 0.1.0")).toBeInTheDocument();
  expect(screen.getByText("Running workers").parentElement).toHaveTextContent("Running workers2");
  expect(screen.getByText("Retained sessions").parentElement).toHaveTextContent("Retained sessions3");
  expect(screen.getByRole("button", { name: "Light meadow" })).toHaveAttribute("aria-pressed", "true");

  fireEvent.click(screen.getByRole("button", { name: "Night hive" }));
  expect(onThemeChange).toHaveBeenCalledWith("dark");
});

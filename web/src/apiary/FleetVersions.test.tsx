import { render, screen } from "@testing-library/react";
import { expect, test } from "vitest";

import type { FleetVersions as Fleet } from "../api";
import FleetVersions from "./FleetVersions";

const hive = (over: Partial<Fleet["hives"][number]>) => ({
  hive_id: "hive-1", node_id: "node-1", swarm_version: "1.12.0",
  database_schema_version: 187, observed_at: 1_000, standing: "current" as const, raises: false, ...over,
});

test("a Hive left behind is announced, not merely listed in a column", () => {
  render(<FleetVersions
    fleet={{
      expected_release: "1.12.0", expected_release_first_seen_at: 500, expected_schema_version: 187,
      hives: [hive({}), hive({ hive_id: "hive-2", node_id: "node-2", swarm_version: "1.11.0", standing: "behind", raises: true })],
    }}
    nameFor={(id) => (id === "hive-2" ? "Paul's Hive" : "Keeper")}
  />);

  // The alert is the point. Both versions being visible is not enough — that is
  // exactly what this Hive had while it sat wedged on a stale build for a day.
  expect(screen.getByRole("alert")).toHaveTextContent("1 Hive is behind what the Apiary expects.");
  expect(screen.getByRole("list", { name: "Swarm versions by Hive" })).toHaveTextContent("Paul's Hive");
  expect(screen.getByText("Behind")).toBeInTheDocument();
  expect(screen.getByText("Current release 1.12.0")).toBeInTheDocument();
});

test("a fleet nobody could compare says so rather than reading as healthy", () => {
  render(<FleetVersions fleet={{
    expected_release: null, expected_release_first_seen_at: null, expected_schema_version: 187,
    hives: [hive({ standing: "unknown" })],
  }} />);

  expect(screen.getByRole("status")).toHaveTextContent("No release check has returned a version yet");
  expect(screen.getByText("Not compared")).toBeInTheDocument();
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  expect(screen.getByText("No release known")).toBeInTheDocument();
});

test("a development build is shown without being called behind", () => {
  render(<FleetVersions fleet={{
    expected_release: "1.13.0", expected_release_first_seen_at: 500, expected_schema_version: 187,
    hives: [hive({ swarm_version: "1.12.0-dev-a597c3ad", standing: "development" })],
  }} />);

  expect(screen.getByText("Development build")).toBeInTheDocument();
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
});

/**
 * ⚠️ A PANEL THAT THROWS TAKES THE WHOLE KEEPER CONTROL ROOM WITH IT. The first
 * version read `fleet.hives` directly and unmounted every other panel when a
 * response did not have the shape it expected — a surface whose job is to raise
 * a problem causing a bigger one.
 */
test("an unshaped or missing response renders empty instead of crashing the room", () => {
  const view = render(<FleetVersions />);
  expect(screen.getByText("No Hive has reported its version yet.")).toBeInTheDocument();
  expect(screen.queryByRole("status")).not.toBeInTheDocument();

  view.rerender(<FleetVersions fleet={[] as unknown as Fleet} />);
  expect(screen.getByText("No Hive has reported its version yet.")).toBeInTheDocument();
});

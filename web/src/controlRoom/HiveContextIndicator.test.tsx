import { render, screen } from "@testing-library/react";
import { expect, test } from "vitest";

import HiveContextIndicator from "./HiveContextIndicator";

test("shows the local Hive before it joins an Apiary", () => {
  render(<HiveContextIndicator identity={{
    operator: { id: "operator-1", display_name: "Bea" },
    hive: { id: "hive-1", name: "Meadow Hive", operator_id: "operator-1", apiary_id: null },
    apiary_context: { mode: "personal" },
  }} />);

  expect(screen.getByLabelText("Meadow Hive is a personal Hive")).toHaveTextContent("Meadow HivePersonal Hive");
});

test("makes the Keeper and Apiary visible without exposing federation details", () => {
  render(<HiveContextIndicator identity={{
    operator: { id: "operator-1", display_name: "Bea" },
    hive: { id: "hive-1", name: "Meadow Hive", operator_id: "operator-1", apiary_id: "apiary-1" },
    apiary_context: {
      mode: "federated",
      apiary: { id: "apiary-1", name: "Grand Garden", keeper_operator_id: "operator-1", shared_work_backend: "jira" },
      local_role: "keeper",
    },
  }} compact />);

  const indicator = screen.getByLabelText("Meadow Hive is the Keeper of Grand Garden");
  expect(indicator).toHaveTextContent("Grand GardenKeeper");
  expect(indicator).not.toHaveTextContent("operator-1");
  expect(indicator).not.toHaveTextContent("jira");
});

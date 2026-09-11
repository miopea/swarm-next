import { render, screen } from "@testing-library/react";
import { expect, test } from "vitest";
import type { FederationCatalogReadiness } from "../api";
import SharedCatalogStatus from "./SharedCatalogStatus";
import { jiraSetupLabel } from "./presentation";

const catalog = { acknowledgement: {}, jira_connection: "not_connected", projects: [], blockers: ["integration_not_ready"] } as unknown as FederationCatalogReadiness;

test("optional Jira does not look like blocked membership or missing shared catalog", () => {
  render(<SharedCatalogStatus catalog={catalog} />);
  expect(screen.queryByRole("list", { name: "Shared work blockers" })).not.toBeInTheDocument();
  expect(screen.getByText("Shared catalog prerequisites are ready.")).toBeInTheDocument();
  expect(screen.getByRole("list", { name: "Jira setup requirements" })).toHaveTextContent("Connect Jira if you want to use Jira work");
  expect(screen.getByRole("link", { name: "Review Jira setup" })).toHaveAttribute("href", "#settings-integrations");
});

test("mixed failures retain shared policy and catalog holds separately from project access", () => {
  const view = render(<SharedCatalogStatus catalog={{ ...catalog, blockers: ["catalog_stale", "policy_revision_changed", "project_access_not_ready"] }} />);
  expect(screen.getByRole("list", { name: "Shared work blockers" })).toHaveTextContent("Keeper catalog needs refreshingApiary policy changed");
  expect(screen.getByRole("list", { name: "Jira setup requirements" })).toHaveTextContent("Project access or workflow mapping is incomplete");
  expect(screen.queryByText("Shared catalog prerequisites are ready.")).not.toBeInTheDocument();
  view.rerender(<SharedCatalogStatus catalog={{ ...catalog, jira_connection: "ready", blockers: [] }} />);
  expect(screen.queryByRole("list")).not.toBeInTheDocument();
  expect(screen.getByText("Shared catalog prerequisites are ready.")).toBeInTheDocument();
});

test("missing observations never imply ready", () => {
  const view = render(<SharedCatalogStatus />);
  expect(screen.getByText("Waiting for shared catalog status.")).toBeInTheDocument();
  view.rerender(<SharedCatalogStatus catalog={{ ...catalog, acknowledgement: null, blockers: [] }} />);
  expect(screen.queryByText("Shared catalog prerequisites are ready.")).not.toBeInTheDocument();
});

test.each([
  [undefined, "Checking"], ["not_connected", "Not connected (optional)"], ["ready", "Connected"],
  ["network_unavailable", "Temporarily unavailable"], ["credentials_invalid", "Sign-in needed"],
  ["permission_denied", "Access needs review"],
] as const)("Jira status %s is distinct", (state, label) => expect(jiraSetupLabel(state)).toBe(label));

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { FederationCatalogReadiness, FederationSyncHealth } from "../api";
import MemberSetup from "./MemberSetup";

afterEach(cleanup);

test.each([
  ["network_unavailable", "Jira is temporarily unreachable", "Check Jira connection"],
  ["credentials_invalid", "Jira sign-in needs attention", "Review Jira sign-in"],
  ["permission_denied", "Jira access needs review", "Review Jira access"],
] as const)("distinguishes %s from first-time setup", (connection, heading, action) => {
  const onRefresh = vi.fn();
  const view = render(<MemberSetup catalog={{ ...catalog, jira_connection: connection }} sync={sync} onManage={vi.fn()} onRefresh={onRefresh} />);
  expect(screen.getByText(heading)).toBeInTheDocument();
  expect(screen.queryByRole("link", { name: "Connect Jira" })).not.toBeInTheDocument();
  if (connection === "network_unavailable") {
    fireEvent.click(screen.getByRole("button", { name: action }));
    expect(onRefresh).toHaveBeenCalledOnce();
  } else expect(screen.getByRole("link", { name: action })).toHaveAttribute("href", "#settings-integrations");
  view.rerender(<MemberSetup catalog={{ ...catalog, jira_connection: "ready" }} sync={sync} onManage={vi.fn()} onRefresh={onRefresh} />);
  expect(screen.queryByText(heading)).not.toBeInTheDocument();
});
const catalog: FederationCatalogReadiness = { acknowledgement: null, jira_connection: "not_connected", projects: [], blockers: [] };
const sync: FederationSyncHealth = { condition: "current", last_attempt_at: 1, last_success_at: 1, consecutive_failures: 0, next_attempt_at: null };

test("optional Jira links to its settings without making membership a failure", () => {
  const view = render(<MemberSetup catalog={catalog} sync={sync} onManage={vi.fn()} onRefresh={vi.fn()} />);
  expect(screen.getByRole("link", { name: "Connect Jira" })).toHaveAttribute("href", "#settings-integrations");
  expect(screen.getByText(/Your Hive is a member/)).toBeInTheDocument();
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  view.rerender(<MemberSetup catalog={{ ...catalog, jira_connection: "ready" }} sync={sync} onManage={vi.fn()} onRefresh={vi.fn()} />);
  expect(screen.queryByRole("link", { name: "Connect Jira" })).not.toBeInTheDocument();
});

test("missing observations do not claim a healthy connection or failed membership", () => {
  render(<MemberSetup onManage={vi.fn()} onRefresh={vi.fn()} />);
  expect(screen.getByRole("status")).toHaveTextContent("Checking shared setup");
  expect(screen.queryByText("Connected to Keeper")).not.toBeInTheDocument();
});

test("connection and policy actions preserve the distinction between refresh and consent", () => {
  const onRefresh = vi.fn();
  const onManage = vi.fn();
  render(<MemberSetup catalog={{ ...catalog, blockers: ["policy_revision_changed"] }}
    sync={{ ...sync, condition: "offline" }} onManage={onManage} onRefresh={onRefresh} />);
  fireEvent.click(screen.getByRole("button", { name: "Check connection" }));
  expect(onRefresh).toHaveBeenCalledOnce();
  expect(onManage).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Review policy" }));
  expect(onManage).toHaveBeenCalledOnce();
});

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

test("adding an inaccessible department preserves ready projects and removes guidance after recovery", () => {
  const ready = {
    project: { project_id: "dev", project_key: "DEV", project_name: "Development" },
    binding_id: "dev-binding", access_verified: true, workflow_mapped: true,
  };
  const department = {
    project: { project_id: "it", project_key: "IT", project_name: "IT" },
    binding_id: null, access_verified: false, workflow_mapped: false,
  };
  const onManage = vi.fn();
  const onRefresh = vi.fn();
  const view = render(<MemberSetup catalog={{ ...catalog, jira_connection: "ready", projects: [ready] }} sync={sync} onManage={onManage} onRefresh={onRefresh} />);
  expect(screen.getByText("1 Jira project ready")).toBeInTheDocument();
  view.rerender(<MemberSetup catalog={{ ...catalog, jira_connection: "ready", projects: [ready, department], blockers: ["project_access_not_ready"] }} sync={sync} onManage={onManage} onRefresh={onRefresh} />);
  expect(screen.getByText("1 Jira project ready")).toBeInTheDocument();
  expect(screen.getByText(/Only configure the projects you use/)).toBeInTheDocument();
  expect(screen.getByText(/Your Hive is a member/)).toBeInTheDocument();
  expect(screen.getByRole("link", { name: "Review Jira projects" })).toHaveAttribute("href", "#settings-integrations");
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  view.rerender(<MemberSetup catalog={{ ...catalog, jira_connection: "ready", projects: [ready, { ...department, binding_id: "it-binding", access_verified: true, workflow_mapped: true }] }} sync={sync} onManage={onManage} onRefresh={onRefresh} />);
  expect(screen.getByText("2 Jira projects ready")).toBeInTheDocument();
  expect(screen.queryByRole("link", { name: "Review Jira projects" })).not.toBeInTheDocument();
  expect(onManage).not.toHaveBeenCalled();
  expect(onRefresh).not.toHaveBeenCalled();
});

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { HiveIdentity } from "../api";
import ApiaryWorkspace from "./ApiaryWorkspace";

vi.mock("./KeeperControlRoom", () => ({ default: ({ onManage, onInvite }: { onManage: () => void; onInvite: () => void }) => <><button onClick={onManage}>Keeper management</button><button onClick={onInvite}>Invite a Hive</button></> }));
vi.mock("./MemberControlRoom", () => ({ default: ({ onManage }: { onManage: () => void }) => <button onClick={onManage}>Member management</button> }));
vi.mock("../settings/ApiarySettings", () => ({ default: ({ initialFocus }: { initialFocus?: string }) => <div data-focus={initialFocus}>Shared configuration</div> }));
afterEach(cleanup);

test.each(["keeper", "member"] as const)("%s manages the Apiary in place and returns to the overview", (role) => {
  const identity: HiveIdentity = {
    operator: { id: "operator", display_name: "Bea" },
    hive: { id: "hive", name: "Hive", operator_id: "operator", apiary_id: "apiary" },
    apiary_context: { mode: "federated", local_role: role,
      apiary: { id: "apiary", name: "Garden", keeper_operator_id: "keeper", shared_work_backend: "jira" } },
  };
  render(<ApiaryWorkspace identity={identity} operatorToken="fictional" busy={false} onIdentityChange={vi.fn()} onOpenTasks={vi.fn()} />);
  const label = role === "keeper" ? "Keeper management" : "Member management";
  fireEvent.click(screen.getByRole("button", { name: label }));
  expect(screen.getByText("Shared configuration")).toBeInTheDocument();
  expect(screen.getByText("Shared configuration").parentElement).toHaveClass("apiary-management");
  expect(screen.queryByRole("button", { name: label })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Back to Apiary overview" }));
  expect(screen.getByRole("button", { name: label })).toBeInTheDocument();
  expect(screen.queryByText("Shared configuration")).not.toBeInTheDocument();
  if (role === "keeper") {
    fireEvent.click(screen.getByRole("button", { name: "Invite a Hive" }));
    expect(screen.getByText("Shared configuration")).toHaveAttribute("data-focus", "invitations");
    fireEvent.click(screen.getByRole("button", { name: "Back to Apiary overview" }));
    fireEvent.click(screen.getByRole("button", { name: label }));
    expect(screen.getByText("Shared configuration")).not.toHaveAttribute("data-focus");
  } else expect(screen.queryByRole("button", { name: "Invite a Hive" })).not.toBeInTheDocument();
});

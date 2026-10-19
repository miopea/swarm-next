import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { ApiaryTask } from "../api";
import SharedTaskWait from "./SharedTaskWait";

const task: ApiaryTask = {
  id: "shared-1",
  apiary_id: "garden",
  source: "swarm",
  title: "Move the backup circuit",
  description: "",
  priority: "normal",
  state: "blocked",
  home_node_id: "node-2",
  home_hive_id: "clover",
  revision: 4,
  created_at: 1,
  updated_at: 2,
};
const names: Record<string, string> = { clover: "Clover Hive", meadow: "Meadow Hive" };
const hiveName = (hiveId: string) => names[hiveId];

describe("SharedTaskWait", () => {
  it("says who outside must act and when the work comes back", () => {
    const back = Date.UTC(2026, 9, 21, 15) / 1000;
    render(<SharedTaskWait task={{ ...task, wait: { kind: "outside", needs: "Restore the circuit", party: "Spectrum", resume_at: back } }} hiveName={hiveName} />);
    const line = screen.getByText(/Spectrum: Restore the circuit, back/);
    expect(line.textContent).toContain(new Date(back * 1000).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" }));
    expect(line.textContent).toMatch(/^Waiting for /);
  });

  it("names the Hive whose operator must act, and says you on that Hive", () => {
    const wait = { kind: "operator_action", needs: "Approve M2 in Entra", hive_id: "clover" };
    const { rerender } = render(<SharedTaskWait task={{ ...task, wait }} localHiveId="meadow" hiveName={hiveName} />);
    expect(screen.getByText(/Clover Hive's operator: Approve M2 in Entra/)).toBeInTheDocument();
    rerender(<SharedTaskWait task={{ ...task, wait }} localHiveId="clover" hiveName={hiveName} />);
    expect(screen.getByText(/you: Approve M2 in Entra/).textContent).toBe("Waiting for you: Approve M2 in Entra");
  });

  it("shows a kind from a later release by its words", () => {
    render(<SharedTaskWait task={{ ...task, wait: { kind: "vendor_ticket", needs: "" } }} hiveName={hiveName} />);
    expect(screen.getByText(/vendor ticket/)).toBeInTheDocument();
  });

  it("says when the home Hive is too old to say why, and nothing otherwise", () => {
    const { container, rerender } = render(<SharedTaskWait task={task} hiveName={hiveName} silentRelease={{ hive: "Thistle Hive", version: "1.17.4" }} />);
    expect(screen.getByText("Thistle Hive runs 1.17.4, which does not say why work is blocked")).toBeInTheDocument();
    rerender(<SharedTaskWait task={task} hiveName={hiveName} />);
    expect(container).toBeEmptyDOMElement();
    rerender(<SharedTaskWait task={{ ...task, state: "active", wait: { kind: "date", needs: "x", resume_at: 5 } }} hiveName={hiveName} />);
    expect(container).toBeEmptyDOMElement();
  });
});

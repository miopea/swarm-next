import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import type { ApiaryTask } from "../api";
import SharedTaskGroups from "./SharedTaskGroups";

afterEach(cleanup);
const task = (state: ApiaryTask["state"]): ApiaryTask => ({
  id: state, apiary_id: "fictional", source: "swarm", title: state,
  description: "", priority: "normal", state, home_node_id: null,
  home_hive_id: null, revision: 1, created_at: 1, updated_at: 1,
});
const props = {
  emptyMessage: "No open work",
  renderTasks: (tasks: ApiaryTask[]) => <ul>{tasks.map((item) => <li key={item.id}>{item.title}</li>)}</ul>,
};

test("keeps all unfinished states visible while closed history is optional", () => {
  const states: ApiaryTask["state"][] = ["draft", "ready", "active", "blocked", "review", "awaiting_release", "completed", "abandoned"];
  render(<SharedTaskGroups {...props} tasks={states.map(task)} />);
  for (const state of states.slice(0, -2)) expect(screen.getByText(state)).toBeVisible();
  expect(screen.queryByText("completed")).not.toBeInTheDocument();
  expect(screen.queryByText("abandoned")).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Show closed shared work · 2" }));
  const history = screen.getByRole("region", { name: "Closed shared work" });
  expect(within(history).getByText("completed")).toBeVisible();
  expect(within(history).getByText("abandoned")).toBeVisible();
  fireEvent.click(screen.getByRole("button", { name: "Hide closed shared work · 2" }));
  expect(screen.queryByRole("region", { name: "Closed shared work" })).not.toBeInTheDocument();
});

test("closure and reopening reclassify the same record without losing history", () => {
  const current = task("ready");
  const { rerender } = render(<SharedTaskGroups {...props} tasks={[current]} />);
  expect(screen.getByText("ready")).toBeVisible();
  rerender(<SharedTaskGroups {...props} tasks={[{ ...current, state: "completed" }]} />);
  expect(screen.getByText("No open work")).toBeVisible();
  expect(screen.queryByText("ready")).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Show closed shared work · 1" }));
  expect(screen.getByText("ready")).toBeVisible();
  rerender(<SharedTaskGroups {...props} tasks={[{ ...current, state: "ready" }]} />);
  expect(screen.getByText("ready")).toBeVisible();
  expect(screen.queryByRole("region", { name: "Closed shared work" })).not.toBeInTheDocument();
  expect(screen.queryByRole("button")).not.toBeInTheDocument();
});

test("an empty snapshot has no history control", () => {
  render(<SharedTaskGroups {...props} tasks={[]} />);
  expect(screen.getByText("No open work")).toBeVisible();
  expect(screen.queryByRole("button")).not.toBeInTheDocument();
});

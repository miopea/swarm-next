import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { changeTaskPrerequisite, type Task } from "../api/tasks";
import { RuntimeRequestError } from "../api/request";
import TaskPrerequisiteDialog from "./TaskPrerequisiteDialog";

vi.mock("../api/tasks", async (original) => ({ ...await original<typeof import("../api/tasks")>(), changeTaskPrerequisite: vi.fn() }));
afterEach(cleanup);
beforeEach(() => { vi.mocked(changeTaskPrerequisite).mockReset(); });
const task: Task = { id: "consumer", hive_id: "hive", title: "Consumer", state: "blocked", description: "", operator_instruction: "", priority: "normal", workspace: "/demo", assigned_worker_id: null, assigned_session_id: null, position: 0, created_at: 1, updated_at: 1 };
const upstream: Task = { ...task, id: "contract", title: "Shared contract", state: "ready" };
const edge = { task_id: task.id, prerequisite_id: upstream.id, title: upstream.title, state: "ready" as const, assigned_worker_id: null, removed: false, reason: "Contract first", created_at: 1 };
function mount(overrides: Partial<React.ComponentProps<typeof TaskPrerequisiteDialog>> = {}) {
  const props = { task, candidates: [task, upstream], operatorToken: "token", onChanged: vi.fn(), onClose: vi.fn(), ...overrides };
  return { ...render(<TaskPrerequisiteDialog {...props} />), props };
}
function choose() {
  fireEvent.change(screen.getByLabelText("Prerequisite task"), { target: { value: upstream.id } });
  fireEvent.change(screen.getByLabelText("Why change this link?"), { target: { value: "Need the agreed contract" } });
}
test("sends one explicit audited change and publishes the returned task", async () => {
  const updated = { ...task, prerequisites: [edge] };
  vi.mocked(changeTaskPrerequisite).mockResolvedValue(updated);
  const { props } = mount();
  expect(screen.getByRole("button", { name: "Add prerequisite" })).toBeDisabled();
  choose();
  fireEvent.click(screen.getByRole("button", { name: "Add prerequisite" }));
  await waitFor(() => expect(props.onChanged).toHaveBeenCalledWith(updated));
  expect(changeTaskPrerequisite).toHaveBeenCalledExactlyOnceWith("token", task.id, { prerequisite_id: upstream.id, operation: "add", reason: "Need the agreed contract" });
  expect(props.onClose).toHaveBeenCalledOnce();
});
test("keeps input after refusal and guards accidental dismissal", async () => {
  vi.mocked(changeTaskPrerequisite).mockRejectedValue(new RuntimeRequestError(409, "This prerequisite would create a dependency cycle"));
  const { props } = mount();
  choose();
  fireEvent.click(screen.getByRole("button", { name: "Add prerequisite" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("dependency cycle");
  expect(screen.getByLabelText("Why change this link?")).toHaveValue("Need the agreed contract");
  fireEvent.keyDown(window, { key: "Escape" });
  expect(screen.getByRole("alertdialog", { name: "Unsaved prerequisite change" })).toBeVisible();
  expect(props.onClose).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Keep editing" }));
  expect(screen.getByLabelText("Prerequisite task")).toHaveValue(upstream.id);
});
test("allows removal of a removed upstream task without changing the source lifecycle", async () => {
  const source = { ...task, state: "active" as const, prerequisites: [{ ...edge, removed: true }] };
  vi.mocked(changeTaskPrerequisite).mockResolvedValue({ ...source, prerequisites: [] });
  const { props } = mount({ task: source, candidates: [] });
  expect(screen.getByRole("option", { name: "Add prerequisite" })).toBeDisabled();
  choose();
  fireEvent.click(screen.getByRole("button", { name: "Remove prerequisite" }));
  await waitFor(() => expect(props.onChanged).toHaveBeenCalledWith({ ...source, prerequisites: [] }));
  expect(changeTaskPrerequisite).toHaveBeenCalledWith("token", task.id, { prerequisite_id: upstream.id, operation: "remove", reason: "Need the agreed contract" });
});
test("bounds candidate options and excludes self and foreign Hive tasks", () => {
  const candidates = Array.from({ length: 70 }, (_, index) => ({ ...upstream, id: `c${index}`, title: `Contract ${index}` }));
  mount({ candidates: [task, { ...upstream, hive_id: "foreign", title: "Foreign contract" }, ...candidates] });
  expect(screen.queryByRole("option", { name: /Foreign contract/ })).not.toBeInTheDocument();
  expect(screen.queryByRole("option", { name: /Consumer/ })).not.toBeInTheDocument();
  expect(screen.getByText(/Showing the first 50 matches/)).toBeVisible();
  fireEvent.change(screen.getByLabelText("Find task"), { target: { value: "Contract 69" } });
  expect(screen.getByRole("option", { name: "Contract 69 · ready" })).toBeInTheDocument();
});
test("does not dismiss or submit twice while the request is pending", async () => {
  let finish!: (task: Task) => void;
  vi.mocked(changeTaskPrerequisite).mockReturnValue(new Promise((resolve) => { finish = resolve; }));
  const { props } = mount();
  choose();
  fireEvent.click(screen.getByRole("button", { name: "Add prerequisite" }));
  fireEvent.keyDown(window, { key: "Escape" });
  expect(props.onClose).not.toHaveBeenCalled();
  expect(screen.getByRole("button", { name: "Saving…" })).toBeDisabled();
  expect(changeTaskPrerequisite).toHaveBeenCalledOnce();
  finish({ ...task, prerequisites: [edge] });
  await waitFor(() => expect(props.onClose).toHaveBeenCalledOnce());
});

test("rechecks task eligibility when live state changes while editing", () => {
  const { props, rerender } = mount();
  choose();
  rerender(<TaskPrerequisiteDialog {...props} task={{ ...task, state: "active" }} />);
  expect(screen.getByRole("button", { name: "Add prerequisite" })).toBeDisabled();
  expect(screen.getByLabelText("Why change this link?")).toHaveValue("Need the agreed contract");
  expect(changeTaskPrerequisite).not.toHaveBeenCalled();
});

test("retains choices without claiming success after an uncertain response", async () => {
  vi.mocked(changeTaskPrerequisite).mockRejectedValue(new TypeError("Network interrupted"));
  const { props } = mount();
  choose();
  fireEvent.click(screen.getByRole("button", { name: "Add prerequisite" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("could not be confirmed");
  expect(screen.getByLabelText("Prerequisite task")).toHaveValue(upstream.id);
  expect(props.onChanged).not.toHaveBeenCalled();
  expect(props.onClose).not.toHaveBeenCalled();
});

test("enforces the server byte limit for multibyte reasons", () => {
  mount();
  choose();
  fireEvent.change(screen.getByLabelText("Why change this link?"), { target: { value: "🐝".repeat(513) } });
  expect(screen.getByRole("alert")).toHaveTextContent("2,048 bytes");
  expect(screen.getByRole("button", { name: "Add prerequisite" })).toBeDisabled();
});

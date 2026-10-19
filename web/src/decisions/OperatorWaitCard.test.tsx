import { fireEvent, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import OperatorWaitCard from "./OperatorWaitCard";

const wait = {
  task_id: "task-phase-e",
  task_title: "SOA Phase E",
  worker_name: "RCG Networks",
  needs: "Sign in to the Entra portal and approve M2",
  waiting_since: 1,
};

test("says who is waiting on the operator and for what, and Done sends it back", () => {
  const onDone = vi.fn();
  const onOpenTask = vi.fn();
  render(<OperatorWaitCard waits={[wait]} onDone={onDone} onOpenTask={onOpenTask} />);
  expect(screen.getByRole("heading", { name: "1 task needs you to do something" })).toBeInTheDocument();
  expect(screen.getByText("RCG Networks is waiting on you:")).toBeInTheDocument();
  expect(screen.getByText(/Sign in to the Entra portal and approve M2/)).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Done" }));
  expect(onDone).toHaveBeenCalledWith("task-phase-e");
  fireEvent.click(screen.getByRole("button", { name: "Open the task" }));
  expect(onOpenTask).toHaveBeenCalledWith("task-phase-e");
});

test("renders nothing when nothing is owed, and Done waits while busy", () => {
  const { container, rerender } = render(<OperatorWaitCard waits={[]} onDone={vi.fn()} />);
  expect(container).toBeEmptyDOMElement();
  rerender(<OperatorWaitCard waits={[wait]} busy onDone={vi.fn()} />);
  expect(screen.getByRole("button", { name: "Done" })).toBeDisabled();
});

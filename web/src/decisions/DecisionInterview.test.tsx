import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import DecisionInterview from "./DecisionInterview";

afterEach(cleanup);

test("a selected custom answer must be completed or deselected before sending", () => {
  const onAnswer = vi.fn();
  render(<DecisionInterview questions={[{ header: "Areas", question: "Which areas?", options: ["Web", "API"], multi_select: true }]} busy={false} onAnswer={onAnswer} />);
  fireEvent.click(screen.getByRole("button", { name: "Web" }));
  fireEvent.click(screen.getByRole("button", { name: "Something else" }));
  const send = screen.getByRole("button", { name: "Send answers" });
  expect(send).toBeDisabled();
  fireEvent.change(screen.getByLabelText("Your answer"), { target: { value: "   " } });
  expect(send).toBeDisabled();
  fireEvent.change(screen.getByLabelText("Your answer"), { target: { value: "Mobile" } });
  expect(send).toBeEnabled();
  fireEvent.click(send);
  expect(onAnswer).toHaveBeenCalledWith({ Areas: ["Web", "Mobile"] }, "");
  fireEvent.change(screen.getByLabelText("Your answer"), { target: { value: "" } });
  expect(send).toBeDisabled();
  fireEvent.click(screen.getByRole("button", { name: "Something else" }));
  expect(send).toBeEnabled();
});

const questions = [
  { header: "Scope", question: "How wide should this go?", options: ["This repo", "Every repo"] },
  { header: "Timing", question: "When?", options: ["Now", "After the release"] },
];

test("preserves drafts across equivalent snapshots and busy refreshes", () => {
  const onAnswer = vi.fn();
  const { rerender } = render(<DecisionInterview questions={questions} busy={false} onAnswer={onAnswer} />);
  fireEvent.click(screen.getByRole("button", { name: "This repo" }));
  fireEvent.click(screen.getAllByRole("button", { name: "Something else" })[1]);
  fireEvent.change(screen.getByLabelText("Your answer"), { target: { value: "Tomorrow" } });
  fireEvent.change(screen.getByLabelText("Anything else the worker should know"), { target: { value: "Keep the tests" } });
  const refreshed = questions.map((question) => ({ ...question, options: [...question.options], multi_select: false }));
  rerender(<DecisionInterview questions={refreshed} busy={true} onAnswer={onAnswer} />);
  expect(screen.getByRole("button", { name: "Send answers" })).toBeDisabled();
  rerender(<DecisionInterview questions={refreshed} busy={false} onAnswer={onAnswer} />);
  fireEvent.click(screen.getByRole("button", { name: "Send answers" }));
  expect(onAnswer).toHaveBeenCalledWith({ Scope: ["This repo"], Timing: ["Tomorrow"] }, "Keep the tests");
});

test.each([
  { label: "wording", next: [{ ...questions[0], question: "Delete which repositories?" }, questions[1]] },
  { label: "options", next: [{ ...questions[0], options: ["One file", "Everything"] }, questions[1]] },
  { label: "option order", next: [{ ...questions[0], options: [...questions[0].options].reverse() }, questions[1]] },
  { label: "selection mode", next: [{ ...questions[0], multi_select: true }, questions[1]] },
  { label: "question order", next: [...questions].reverse() },
])("requires fresh answers when $label changes, without resurrecting old drafts", ({ next }) => {
  const onAnswer = vi.fn();
  const { rerender } = render(<DecisionInterview questions={questions} busy={false} onAnswer={onAnswer} />);
  fireEvent.click(screen.getByRole("button", { name: "This repo" }));
  fireEvent.click(screen.getAllByRole("button", { name: "Something else" })[1]);
  fireEvent.change(screen.getByLabelText("Your answer"), { target: { value: "Tomorrow" } });
  fireEvent.change(screen.getByLabelText("Anything else the worker should know"), { target: { value: "Old note" } });
  rerender(<DecisionInterview questions={next} busy={false} onAnswer={onAnswer} />);
  expect(screen.getByRole("button", { name: "Send answers" })).toBeDisabled();
  expect(screen.getByLabelText("Anything else the worker should know")).toHaveValue("");
  expect(screen.queryByLabelText("Your answer")).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Send answers" }));
  expect(onAnswer).not.toHaveBeenCalled();
  rerender(<DecisionInterview questions={questions} busy={false} onAnswer={onAnswer} />);
  expect(screen.getByRole("button", { name: "This repo" })).toHaveAttribute("aria-pressed", "false");
  expect(screen.getByRole("button", { name: "Send answers" })).toBeDisabled();
});

test("will not send until every question is answered", () => {
  // The asking worker holds its session for the whole set. Half an answer
  // resumes it with an incomplete picture and no way to ask for the rest.
  const onAnswer = vi.fn();
  render(<DecisionInterview questions={questions} busy={false} onAnswer={onAnswer} />);

  const send = screen.getByRole("button", { name: "Send answers" });
  expect(send).toBeDisabled();
  expect(screen.getByRole("status")).toHaveTextContent("2 of 2 still to answer: Scope, Timing");

  fireEvent.click(screen.getByRole("button", { name: "This repo" }));
  expect(send).toBeDisabled();
  expect(screen.getByRole("status")).toHaveTextContent("1 of 2 still to answer: Timing");

  fireEvent.click(screen.getByRole("button", { name: "Now" }));
  expect(send).toBeEnabled();
  fireEvent.click(send);
  expect(onAnswer).toHaveBeenCalledWith({ Scope: ["This repo"], Timing: ["Now"] }, "");
});

test("carries an answer the asker never offered", () => {
  // This is the case the asker failed to guess, which is the reason interviews
  // exist. It must reach the worker as the operator wrote it.
  const onAnswer = vi.fn();
  render(<DecisionInterview questions={questions} busy={false} onAnswer={onAnswer} />);

  fireEvent.click(screen.getByRole("button", { name: "This repo" }));
  fireEvent.click(screen.getAllByRole("button", { name: "Something else" })[1]);
  fireEvent.change(screen.getByLabelText("Your answer"), {
    target: { value: "After the Jira mapping is fixed" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Send answers" }));

  expect(onAnswer).toHaveBeenCalledWith(
    { Scope: ["This repo"], Timing: ["After the Jira mapping is fixed"] },
    "",
  );
});

test("keeps a single-choice question to one answer and lets multi-select hold several", () => {
  const onAnswer = vi.fn();
  render(<DecisionInterview
    questions={[
      { header: "Scope", question: "How wide?", options: ["A", "B"] },
      { header: "Areas", question: "Which areas?", options: ["Web", "API"], multi_select: true },
    ]}
    busy={false}
    onAnswer={onAnswer}
  />);

  fireEvent.click(screen.getByRole("button", { name: "A" }));
  fireEvent.click(screen.getByRole("button", { name: "B" }));
  fireEvent.click(screen.getByRole("button", { name: "Web" }));
  fireEvent.click(screen.getByRole("button", { name: "API" }));
  fireEvent.click(screen.getByRole("button", { name: "Send answers" }));

  expect(onAnswer).toHaveBeenCalledWith({ Scope: ["B"], Areas: ["Web", "API"] }, "");
});

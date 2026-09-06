import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import DecisionAnswerFixture from "./DecisionAnswerFixture";

afterEach(cleanup);

test("failed custom answer stays editable and retry records exact operator text", async () => {
  render(<DecisionAnswerFixture />);
  fireEvent.click(screen.getByRole("button", { name: "Say something else" }));
  const text = "Keep the existing values.\nPrepare a preview before changing anything.";
  fireEvent.change(screen.getByRole("textbox", { name: "Tell the worker what to do instead" }), { target: { value: text } });
  fireEvent.click(screen.getByRole("button", { name: "Send this instead" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("still here to retry");
  expect(screen.getByRole("textbox", { name: "Tell the worker what to do instead" })).toHaveValue(text);
  expect(screen.queryByLabelText("Recorded fixture answer")).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Send this instead" }));
  await waitFor(() => expect(screen.getByLabelText("Recorded fixture answer").textContent).toBe(JSON.stringify({ answer: text, note: "" })));
  expect(screen.queryByRole("button", { name: "Send this instead" })).not.toBeInTheDocument();
});

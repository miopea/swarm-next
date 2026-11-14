import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

import type { DecisionClarification, DecisionImage, DecisionRequest } from "../api";
import DecisionInbox from "./DecisionInbox";
import { MAX_ANSWER_IMAGE_BYTES, needsShrinking } from "./answerImages";

const PNG_BYTES = "\x89PNG\r\n\x1a\nfictional";
const PNG_BASE64 = btoa(PNG_BYTES);

function png(name = "shot.png") {
  return new File([Uint8Array.from(PNG_BYTES, (character) => character.charCodeAt(0))], name, { type: "image/png" });
}

function paste(target: HTMLElement, files: File[]) {
  fireEvent.paste(target, { clipboardData: { files, getData: () => "" } });
}

const pending: DecisionRequest = {
  id: "decision-1",
  hive_id: "hive-1",
  requesting_worker_id: "worker-1",
  task_id: null,
  kind: "input",
  urgency: "normal",
  title: "Choose the layout",
  reason: "Two layouts remain",
  risk: "",
  evidence: "",
  summary: "Which layout to keep.",
  suggested_action: "",
  allowed_actions: ["keep_it", "change_it"],
  deadline: null,
  state: "pending",
  resolution_action: null,
  resolution_note: "",
  resolved_by_operator_id: null,
  created_at: 1,
  updated_at: 1,
  resolved_at: null,
  delivery_state: null,
};

const sent = [{ media_type: "image/png", data: PNG_BASE64 }];

let created: string[] = [];
beforeEach(() => {
  created = [];
  Object.defineProperty(URL, "createObjectURL", {
    configurable: true,
    value: vi.fn(() => { const url = `blob:fictional-${created.length}`; created.push(url); return url; }),
  });
  Object.defineProperty(URL, "revokeObjectURL", { configurable: true, value: vi.fn() });
});
afterEach(() => { cleanup(); vi.restoreAllMocks(); });

test("a pasted screenshot goes with the note when the operator rules", async () => {
  const onResolve = vi.fn().mockResolvedValue(undefined);
  render(<DecisionInbox decisions={[pending]} tasks={[]} workers={[]} busy={false} onResolve={onResolve} />);
  fireEvent.click(screen.getByText("Add an optional note"));

  paste(screen.getByRole("textbox", { name: "Optional note" }), [png()]);

  expect(await screen.findByRole("img", { name: "Screenshot 1: shot.png" })).toHaveAttribute("src", `data:image/png;base64,${PNG_BASE64}`);
  expect(screen.getByText(/Edit your note · 1 screenshot/)).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Change it" }));
  await waitFor(() => expect(onResolve).toHaveBeenCalledWith(pending, "change_it", "", "inbox_action", sent));
});

test("a ruling with no screenshot is sent exactly as before", async () => {
  const onResolve = vi.fn().mockResolvedValue(undefined);
  render(<DecisionInbox decisions={[pending]} tasks={[]} workers={[]} busy={false} onResolve={onResolve} />);

  fireEvent.click(screen.getByRole("button", { name: "Keep it" }));

  await waitFor(() => expect(onResolve).toHaveBeenCalledWith(pending, "keep_it", "", "inbox_action"));
});

test("an answer in the operator's own words carries its screenshots and the note's, four at most", async () => {
  const onAnswer = vi.fn().mockResolvedValue(undefined);
  const { container } = render(<DecisionInbox decisions={[pending]} tasks={[]} workers={[]} busy={false} onResolve={vi.fn()} onAnswer={onAnswer} />);
  fireEvent.click(screen.getByText("Add an optional note"));
  paste(screen.getByRole("textbox", { name: "Optional note" }), [png("note.png")]);
  await screen.findByRole("img", { name: "Screenshot 1: note.png" });
  fireEvent.click(screen.getByRole("button", { name: "Say something else" }));
  const words = screen.getByRole("group", { name: "Answer in your own words" });
  const picker = words.querySelector<HTMLInputElement>("input[type=file]")!;

  fireEvent.change(picker, { target: { files: [png("a.png"), png("b.png"), png("c.png"), png("d.png")] } });

  expect(await within(words).findByRole("alert")).toHaveTextContent("Only 3 more fit with this answer; the rest were left out.");
  await waitFor(() => expect(within(words).getAllByRole("img")).toHaveLength(3));
  expect(within(words).getByRole("button", { name: "Attach screenshot" })).toBeDisabled();
  expect(container.querySelector(".decision-note")!.querySelector("button")).toBeDisabled();
  fireEvent.change(within(words).getByRole("textbox", { name: "Tell the worker what to do instead" }), { target: { value: "Move it left, like these" } });
  fireEvent.click(within(words).getByRole("button", { name: "Remove screenshot 2" }));
  fireEvent.click(screen.getByRole("button", { name: "Send this instead" }));

  await waitFor(() => expect(onAnswer).toHaveBeenCalledTimes(1));
  const [, answers, note, images] = onAnswer.mock.calls[0];
  expect(answers).toEqual({ Answer: ["Move it left, like these"] });
  expect(note).toBe("");
  expect(images).toEqual([...sent, ...sent, ...sent]);
});

test("an image the worker cannot be shown is refused with a reason", async () => {
  const onResolve = vi.fn().mockResolvedValue(undefined);
  render(<DecisionInbox decisions={[pending]} tasks={[]} workers={[]} busy={false} onResolve={onResolve} />);
  fireEvent.click(screen.getByText("Add an optional note"));

  paste(screen.getByRole("textbox", { name: "Optional note" }), [new File(["BM"], "old.bmp", { type: "image/bmp" })]);

  expect(await screen.findByRole("alert")).toHaveTextContent("This image could not be read. Try a PNG or JPEG screenshot.");
  expect(screen.queryByRole("img", { name: /Screenshot 1/ })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Keep it" }));
  await waitFor(() => expect(onResolve).toHaveBeenCalledWith(pending, "keep_it", "", "inbox_action"));
});

test("pasting text alone pastes text", () => {
  render(<DecisionInbox decisions={[pending]} tasks={[]} workers={[]} busy={false} onResolve={vi.fn()} />);
  fireEvent.click(screen.getByText("Add an optional note"));
  const note = screen.getByRole("textbox", { name: "Optional note" });

  const event = new Event("paste", { bubbles: true, cancelable: true });
  Object.defineProperty(event, "clipboardData", { value: { files: [], getData: () => "words" } });
  note.dispatchEvent(event);

  expect(event.defaultPrevented).toBe(false);
  expect(screen.queryByRole("img")).not.toBeInTheDocument();
});

test("an interview's note carries its screenshots", async () => {
  const onAnswer = vi.fn().mockResolvedValue(undefined);
  const interview: DecisionRequest = {
    ...pending,
    allowed_actions: [],
    questions: [{ header: "Scope", question: "How wide?", options: ["This page", "Every page"] }],
  };
  render(<DecisionInbox decisions={[interview]} tasks={[]} workers={[]} busy={false} onResolve={vi.fn()} onAnswer={onAnswer} />);
  fireEvent.click(screen.getByRole("button", { name: "Every page" }));
  fireEvent.click(screen.getByText("Add an optional note"));

  paste(screen.getByRole("textbox", { name: "Anything else the worker should know" }), [png()]);
  await screen.findByRole("img", { name: "Screenshot 1: shot.png" });
  fireEvent.click(screen.getByRole("button", { name: "Send answers" }));

  await waitFor(() => expect(onAnswer).toHaveBeenCalledWith(interview, { Scope: ["Every page"] }, "", sent));
});

test("a question carries its screenshots, and the history shows each question's own", async () => {
  const saved: DecisionClarification = {
    id: "round-1", decision_id: pending.id, operator_id: "operator", question: "This one?", asked_at: 10,
    reply: null, replied_at: null, replying_worker_id: null, replying_session_id: null, delivery_state: "queued", image_count: 1,
  };
  const ask = vi.fn().mockResolvedValue(saved);
  const read = vi.fn().mockResolvedValue([saved]);
  const image: DecisionImage = { id: "image-1", decision_id: pending.id, clarification_id: "round-1", media_type: "image/png", byte_len: 18, attached_at: 10 };
  const onFetchImage = vi.fn().mockResolvedValue(new Blob([PNG_BYTES], { type: "image/png" }));
  const props = { tasks: [], workers: [], busy: false, onResolve: vi.fn(), onAskClarification: ask, onFetchClarifications: read, onFetchImage };
  const { rerender } = render(<DecisionInbox {...props} decisions={[pending]} />);
  fireEvent.click(screen.getByRole("button", { name: "Ask a question" }));
  const box = screen.getByLabelText("What would you like clarified?");
  fireEvent.change(box, { target: { value: "This one?" } });
  paste(box, [png()]);
  await screen.findByRole("img", { name: "Screenshot 1: shot.png" });

  fireEvent.click(screen.getByRole("button", { name: "Send question" }));

  await waitFor(() => expect(ask).toHaveBeenCalledTimes(1));
  expect(ask.mock.calls[0][2]).toBe("This one?");
  expect(ask.mock.calls[0][3]).toEqual(sent);
  rerender(<DecisionInbox {...props} decisions={[{ ...pending, images: [image], clarification: {
    round_count: 1, waiting_clarification_id: "round-1", delivery_state: "queued", latest_reply_at: null, next_move: "requester" } }]} />);
  fireEvent.click(screen.getByText(/Questions & replies/));
  expect(await screen.findByRole("img", { name: "Sent screenshot 1" })).toHaveAttribute("src", "blob:fictional-0");
  expect(onFetchImage).toHaveBeenCalledWith(pending.id, "image-1", expect.any(AbortSignal));
});

test("an answered card shows the screenshots sent with the answer, not a question's", async () => {
  const answered: DecisionRequest = {
    ...pending, state: "resolved", resolution_action: "change_it", resolution_note: "Like this", delivery_state: "delivered", resolved_at: 2,
    images: [
      { id: "image-1", decision_id: pending.id, clarification_id: null, media_type: "image/png", byte_len: 18, attached_at: 2 },
      { id: "image-2", decision_id: pending.id, clarification_id: "round-1", media_type: "image/png", byte_len: 18, attached_at: 1 },
    ],
  };
  const onFetchImage = vi.fn().mockResolvedValue(new Blob([PNG_BYTES], { type: "image/png" }));
  render(<DecisionInbox decisions={[answered]} tasks={[]} workers={[]} busy={false} onResolve={vi.fn()} onFetchImage={onFetchImage} />);
  fireEvent.click(screen.getByRole("checkbox", { name: "Show history" }));

  const list = await screen.findByRole("list", { name: "Screenshots you sent with this answer" });
  expect(await within(list).findAllByRole("img")).toHaveLength(1);
  expect(onFetchImage).toHaveBeenCalledTimes(1);
  expect(onFetchImage).toHaveBeenCalledWith(pending.id, "image-1", expect.any(AbortSignal));
});

test("an answered card without a way to read images says how many went", () => {
  const answered: DecisionRequest = {
    ...pending, state: "resolved", resolution_action: "change_it", delivery_state: "delivered", resolved_at: 2,
    images: [{ id: "image-1", decision_id: pending.id, clarification_id: null, media_type: "image/png", byte_len: 18, attached_at: 2 }],
  };
  render(<DecisionInbox decisions={[answered]} tasks={[]} workers={[]} busy={false} onResolve={vi.fn()} />);
  fireEvent.click(screen.getByRole("checkbox", { name: "Show history" }));

  expect(screen.getByText("1 screenshot sent")).toBeInTheDocument();
});

test("only an image the worker cannot take is redrawn", () => {
  expect(needsShrinking("image/png", 1_000, 1920, 1080)).toBe(false);
  expect(needsShrinking("image/webp", MAX_ANSWER_IMAGE_BYTES)).toBe(false);
  expect(needsShrinking("image/png", MAX_ANSWER_IMAGE_BYTES + 1)).toBe(true);
  expect(needsShrinking("image/png", 1_000, 9000, 1000)).toBe(true);
  expect(needsShrinking("image/bmp", 1_000)).toBe(true);
  expect(needsShrinking("image/avif", 1_000)).toBe(true);
});

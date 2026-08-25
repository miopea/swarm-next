import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";

import ImageViewer from "./ImageViewer";

afterEach(cleanup);

test("an attached image announces that it can be opened, and opens", () => {
  // The reported case: a 338 KB phone screenshot rendered at roughly 175px, its
  // text unreadable, with nothing on it saying it could be made bigger.
  render(<ImageViewer src="/api/v1/tasks/t/email/attachments/x.png" filename="Screenshot_20260821-093423.png" />);

  const trigger = screen.getByRole("button", { name: "View Screenshot_20260821-093423.png at full size" });
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();

  fireEvent.click(trigger);

  const dialog = screen.getByRole("dialog", { name: "Screenshot_20260821-093423.png" });
  // Same bytes, not a second fetch and not a download: the ask was to look at
  // it without leaving the page.
  expect(dialog.querySelector("img")).toHaveAttribute("src", "/api/v1/tasks/t/email/attachments/x.png");
  expect(dialog.querySelector("a[download]")).toBeNull();
});

test("Escape closes it and focus goes back to the image it was opened from", () => {
  render(<ImageViewer src="/x.png" filename="x.png" />);
  const trigger = screen.getByRole("button", { name: "View x.png at full size" });
  trigger.focus();

  fireEvent.click(trigger);
  expect(screen.getByRole("dialog")).toBeInTheDocument();

  fireEvent.keyDown(window, { key: "Escape" });

  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  expect(document.activeElement).toBe(trigger);
});

test("the overlay is portalled out of the panel that owns the thumbnail", () => {
  // The email panel is narrow and shares the screen with the task form. An
  // image at full size inside it would push that layout around.
  const { container } = render(<ImageViewer src="/x.png" filename="x.png" />);
  fireEvent.click(screen.getByRole("button", { name: "View x.png at full size" }));

  expect(container.querySelector("[role='dialog']")).toBeNull();
  expect(document.body.querySelector("[role='dialog']")).not.toBeNull();
});

test("clicking the backdrop closes, clicking the image does not", () => {
  render(<ImageViewer src="/x.png" filename="x.png" />);
  fireEvent.click(screen.getByRole("button", { name: "View x.png at full size" }));

  fireEvent.click(screen.getByRole("dialog"));
  expect(screen.getByRole("dialog")).toBeInTheDocument();

  fireEvent.click(document.querySelector(".image-viewer-backdrop")!);
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
});

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import DailyBackupNotice from "./DailyBackupNotice";
afterEach(cleanup);
test("failure opens backup details and recovery quietly removes the notice", () => {
  const onDetails = vi.fn();
  const { rerender } = render(<DailyBackupNotice status={{ state: "failed" }} onDetails={onDetails} />);
  expect(screen.getByRole("status")).toHaveTextContent("Retained backups were left in place");
  fireEvent.click(screen.getByRole("button", { name: "Backup details" }));
  expect(onDetails).toHaveBeenCalledOnce();
  rerender(<DailyBackupNotice status={{ state: "ready", snapshot_day: "20260904" }} onDetails={onDetails} />);
  expect(screen.queryByRole("status")).toBeNull();
});
test("unavailable is not failed and older APIs do not invent backup health", () => {
  const { rerender } = render(<DailyBackupNotice status={{ state: "unavailable" }} onDetails={() => {}} />);
  expect(screen.getByRole("status")).toHaveTextContent("health is unconfirmed");
  rerender(<DailyBackupNotice onDetails={() => {}} />);
  expect(screen.queryByRole("status")).toBeNull();
});

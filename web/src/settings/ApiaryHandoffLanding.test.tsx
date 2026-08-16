import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import { createApiaryHandoffLink, takeStagedApiaryHandoff } from "./apiaryHandoff";
import ApiaryHandoffLanding from "./ApiaryHandoffLanding";

afterEach(() => { cleanup(); window.history.replaceState(null, "", "/"); vi.restoreAllMocks(); });

test("explains how a receiving personal Hive ingests a private Keeper link", () => {
  const link = createApiaryHandoffLink("keeper", { link_id: "link-1", keeper_endpoint: "https://keeper.example.test", secret: "private" }, window.location.origin);
  window.history.replaceState(null, "", link);
  render(<ApiaryHandoffLanding><div>Control room</div></ApiaryHandoffLanding>);

  expect(screen.getByRole("heading", { name: "Open this in your personal Hive" })).toBeInTheDocument();
  expect(screen.getByText(/never sent to a handoff service/i)).toBeInTheDocument();
  expect(screen.queryByText("Control room")).not.toBeInTheDocument();
});

test("hands the fragment to this Hive in memory and clears it from browser history", () => {
  const link = createApiaryHandoffLink("keeper", { link_id: "link-1", keeper_endpoint: "https://keeper.example.test", secret: "private" }, window.location.origin);
  window.history.replaceState(null, "", link);
  render(<ApiaryHandoffLanding><div>Control room</div></ApiaryHandoffLanding>);

  fireEvent.click(screen.getByRole("button", { name: "Use this Hive" }));
  expect(screen.getByText("Control room")).toBeInTheDocument();
  expect(window.location.hash).toBe("#settings-apiary");
  expect(takeStagedApiaryHandoff("keeper")).toBe(link);
});

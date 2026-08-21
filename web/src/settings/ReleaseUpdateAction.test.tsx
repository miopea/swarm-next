import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

import type { ReleaseStatus } from "../api";
import ReleaseUpdateAction from "./ReleaseUpdateAction";

vi.mock("../api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../api")>()),
  fetchReleaseStatus: vi.fn(),
  setReleaseCheckMode: vi.fn(),
  checkForRelease: vi.fn(),
  downloadRelease: vi.fn(),
  applyRelease: vi.fn(),
}));

const api = await import("../api");

function status(overrides: Partial<ReleaseStatus> = {}): ReleaseStatus {
  return {
    available: true,
    mode: "daily",
    current_version: "0.1.0",
    development_build: false,
    last_checked_at: 1_755_800_000,
    last_outcome: "offered",
    offer: {
      version: "0.2.0",
      protocol: "7",
      artifact_url: "https://releases.example/swarm-0.2.0.tar.gz",
      artifact_sha256: "a".repeat(64),
      artifact_bytes: 4096,
      worker_engine_build_id: "engine-a",
      notes_url: null,
    },
    upgrade_available: true,
    stops_workers: false,
    downloaded_version: null,
    ...overrides,
  };
}

beforeEach(() => {
  vi.mocked(api.fetchReleaseStatus).mockReset();
  vi.mocked(api.setReleaseCheckMode).mockReset();
  vi.mocked(api.checkForRelease).mockReset();
  vi.mocked(api.downloadRelease).mockReset();
  vi.mocked(api.applyRelease).mockReset();
});
afterEach(cleanup);

/** "A Hive never contacts an origin its owner did not choose." */
test("asks once before this Hive ever contacts an origin", async () => {
  vi.mocked(api.fetchReleaseStatus).mockResolvedValue(status({ mode: "unset", offer: null, upgrade_available: false, last_checked_at: null, last_outcome: null }));
  vi.mocked(api.setReleaseCheckMode).mockResolvedValue(status({ mode: "daily" }));
  render(<ReleaseUpdateAction busy={false} operatorToken="token" />);

  expect(await screen.findByText("Check for new Swarm releases?")).toBeInTheDocument();
  expect(screen.getByText(/Until you choose, this Hive contacts nothing/)).toBeInTheDocument();
  expect(screen.getByText(/sends nothing — no version, no identity, no counts/)).toBeInTheDocument();
  expect(api.checkForRelease).not.toHaveBeenCalled();

  fireEvent.click(screen.getByRole("button", { name: "Check daily" }));
  await waitFor(() => expect(api.setReleaseCheckMode).toHaveBeenCalledWith("token", "daily"));
});

/** A build with no verifying key or no origin has an inert path, not a broken one. */
test("stays out of the way entirely when this build cannot check", async () => {
  vi.mocked(api.fetchReleaseStatus).mockResolvedValue(status({ available: false }));
  const { container } = render(<ReleaseUpdateAction busy={false} operatorToken="token" />);
  await waitFor(() => expect(api.fetchReleaseStatus).toHaveBeenCalled());
  expect(container).toBeEmptyDOMElement();
});

/**
 * ADR 0050 point 5, which is item 22's lesson encoded: the engine consequence
 * is stated at the moment of consent, not discovered by a timer afterwards.
 */
test("says whether installing stops workers before asking to install", async () => {
  vi.mocked(api.fetchReleaseStatus).mockResolvedValue(status({ stops_workers: true, downloaded_version: "0.2.0" }));
  render(<ReleaseUpdateAction busy={false} operatorToken="token" />);

  expect(await screen.findByText("Stops workers")).toBeInTheDocument();
  expect(screen.getByText(/Installing it stops every running worker/)).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "Install Swarm 0.2.0" }));
  expect(screen.getByText("Install Swarm 0.2.0 now?")).toBeInTheDocument();
  expect(screen.getByText(/Every running worker stops and is brought back/)).toBeInTheDocument();
  expect(api.applyRelease).not.toHaveBeenCalled();

  fireEvent.click(screen.getByRole("button", { name: "Install 0.2.0" }));
  await waitFor(() => expect(api.applyRelease).toHaveBeenCalledWith("token"));
});

/** Downloading is reversible and installing is not, so they are two consents. */
test("downloads and installs as two separate acts", async () => {
  vi.mocked(api.fetchReleaseStatus).mockResolvedValue(status());
  vi.mocked(api.downloadRelease).mockResolvedValue(status({ downloaded_version: "0.2.0" }));
  render(<ReleaseUpdateAction busy={false} operatorToken="token" />);

  fireEvent.click(await screen.findByRole("button", { name: "Download Swarm 0.2.0" }));
  await waitFor(() => expect(api.downloadRelease).toHaveBeenCalledWith("token"));
  expect(api.applyRelease).not.toHaveBeenCalled();
  expect(await screen.findByText(/verified against the signed digest/)).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Install Swarm 0.2.0" })).toBeInTheDocument();
});

/** "Replacing someone's checkout-built binary would discard work whose contents nothing can enumerate." */
test("tells a working copy a release exists and offers it nothing", async () => {
  vi.mocked(api.fetchReleaseStatus).mockResolvedValue(status({ development_build: true, upgrade_available: false }));
  render(<ReleaseUpdateAction busy={false} operatorToken="token" />);

  expect(await screen.findByText("This Hive builds from a working copy")).toBeInTheDocument();
  expect(screen.getByText(/Version 0.2.0 has been released/)).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /Download/ })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /Install/ })).not.toBeInTheDocument();
});

/** An origin unreachable today does not make yesterday's answer untrue. */
test("says a check failed without pretending nothing is available", async () => {
  vi.mocked(api.fetchReleaseStatus).mockResolvedValue(status({ last_outcome: "unreachable" }));
  render(<ReleaseUpdateAction busy={false} operatorToken="token" />);

  expect(await screen.findByText(/could not reach the origin/)).toBeInTheDocument();
  expect(screen.getByText("Swarm 0.2.0 is available")).toBeInTheDocument();
});

/** A manifest that fails verification is ignored, and the operator is told so. */
test("reports a manifest it could not verify rather than acting on it", async () => {
  vi.mocked(api.fetchReleaseStatus).mockResolvedValue(status({ last_outcome: "rejected", offer: null, upgrade_available: false }));
  render(<ReleaseUpdateAction busy={false} operatorToken="token" />);

  expect(await screen.findByText(/found a manifest it could not verify, and ignored it/)).toBeInTheDocument();
});

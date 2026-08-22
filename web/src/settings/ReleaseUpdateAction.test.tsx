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
    carries_new_worker_engine: false,
    downloaded_version: null,
    apply_state: null,
    apply_reason: null,
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
 * The point of separating the terminal host from the API is that an app update
 * does not stop workers, and `swarm-package update` preserves the running host
 * exactly so. This card used to claim the opposite, which the operator caught:
 * "The whole point of our breaking between worker engine and app is to make
 * sure that doesn't happen."
 */
test("promises workers stay online, and defers the engine rather than stopping them", async () => {
  vi.mocked(api.fetchReleaseStatus).mockResolvedValue(
    status({ carries_new_worker_engine: true, downloaded_version: "0.2.0" }),
  );
  render(<ReleaseUpdateAction busy={false} operatorToken="token" />);

  expect(await screen.findByText("Workers stay online")).toBeInTheDocument();
  expect(screen.queryByText(/stops every running worker/)).not.toBeInTheDocument();
  expect(screen.getByText(/Your workers keep running/)).toBeInTheDocument();
  expect(screen.getByText(/deferred while any worker is running/)).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "Install Swarm 0.2.0" }));
  expect(screen.getByText("Install Swarm 0.2.0 now?")).toBeInTheDocument();
  expect(screen.getByText(/The newer worker engine waits until they are idle/)).toBeInTheDocument();
  expect(api.applyRelease).not.toHaveBeenCalled();

  fireEvent.click(screen.getByRole("button", { name: "Install 0.2.0" }));
  await waitFor(() => expect(api.applyRelease).toHaveBeenCalledWith("token"));
});

/**
 * A result from an earlier attempt must not be reported as the current one.
 * "I did check now and it IMMEDIATELY comes back with this" — a failure
 * recorded against 0.2.0 hours earlier, shown as though Install had just been
 * pressed on 0.5.0.
 */
test("ignores an install failure recorded against a different release", async () => {
  vi.mocked(api.fetchReleaseStatus).mockResolvedValue(
    // The API only reports apply_state when it names the offered release, so a
    // stale one arrives as null.
    status({ downloaded_version: "0.2.0", apply_state: null }),
  );
  render(<ReleaseUpdateAction busy={false} operatorToken="token" />);

  expect(await screen.findByText("Swarm 0.2.0 is available")).toBeInTheDocument();
  expect(screen.queryByText(/The install did not run/)).not.toBeInTheDocument();
});

/**
 * "I am still sitting here after 60 seconds... if I refresh the page it goes
 * back to asking me to install." A silent failure that reverts to offering the
 * release is indistinguishable from nothing having happened.
 */
test("says the install did not run rather than quietly offering it again", async () => {
  vi.mocked(api.fetchReleaseStatus).mockResolvedValue(
    status({ downloaded_version: "0.2.0", apply_state: "failed" }),
  );
  render(<ReleaseUpdateAction busy={false} operatorToken="token" />);

  expect(await screen.findByText(/The install did not run/)).toBeInTheDocument();
  expect(screen.getByText(/still on 0.1.0/)).toBeInTheDocument();
  expect(screen.getByText(/swarm-release-apply.service/)).toBeInTheDocument();
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

/**
 * "Seems a bit like the update system is confused if I am in dev or production
 * installs." A Hive that builds from a working copy was asked whether to check
 * for releases with no hint that a release would never be offered to it — the
 * question sat next to the App and API card, which is where its updates
 * actually come from.
 */
test("tells a working copy what checking would and would not do for it", async () => {
  vi.mocked(api.fetchReleaseStatus).mockResolvedValue(
    status({ mode: "unset", development_build: true, offer: null, upgrade_available: false }),
  );
  render(<ReleaseUpdateAction busy={false} operatorToken="token" />);

  expect(await screen.findByText("Check for new Swarm releases?")).toBeInTheDocument();
  expect(screen.getByText(/This Hive builds from a working copy/)).toBeInTheDocument();
  expect(screen.getByText(/never offer to install one/)).toBeInTheDocument();
  expect(screen.getByText(/Your updates come from the App and API card/)).toBeInTheDocument();
});

test("does not say that to an install running a release", async () => {
  vi.mocked(api.fetchReleaseStatus).mockResolvedValue(
    status({ mode: "unset", development_build: false, offer: null, upgrade_available: false }),
  );
  render(<ReleaseUpdateAction busy={false} operatorToken="token" />);

  expect(await screen.findByText("Check for new Swarm releases?")).toBeInTheDocument();
  expect(screen.queryByText(/builds from a working copy/)).not.toBeInTheDocument();
});

/** A refusal records why. Reaching the operator without it made a specific
 *  failure read as a generic one. */
test("says why the install unit refused, not just that it did", async () => {
  vi.mocked(api.fetchReleaseStatus).mockResolvedValue(
    status({ downloaded_version: "0.2.0", apply_state: "refused", apply_reason: "not-a-release" }),
  );
  render(<ReleaseUpdateAction busy={false} operatorToken="token" />);

  expect(await screen.findByText(/The install did not run/)).toBeInTheDocument();
  expect(screen.getByText(/is not a Swarm release/)).toBeInTheDocument();
});

/**
 * "It is really hard to tell if the install finished or not... It installed but
 * just didn't refresh." The card read status once on mount, so "Installing"
 * stayed on screen whatever happened and a finished install looked identical to
 * a stalled one.
 */
test("follows an install through the API restart and says when it arrived", async () => {
  vi.mocked(api.applyRelease).mockResolvedValue(undefined);
  const offered = status({ downloaded_version: "0.6.2" });
  offered.offer = { ...offered.offer!, version: "0.6.2" };
  vi.mocked(api.fetchReleaseStatus)
    .mockResolvedValueOnce(offered)
    // The API goes away to install: the expected middle, not a fault.
    .mockRejectedValueOnce(new Error("connection refused"))
    // It comes back as the new version.
    .mockResolvedValue(
      status({ current_version: "0.6.2", offer: null, upgrade_available: false, downloaded_version: null }),
    );

  render(<ReleaseUpdateAction busy={false} operatorToken="token" />);
  fireEvent.click(await screen.findByRole("button", { name: "Install Swarm 0.6.2" }));
  fireEvent.click(screen.getByRole("button", { name: "Install 0.6.2" }));

  expect(await screen.findByText(/Swarm is restarting to finish the install/, {}, { timeout: 4000 }))
    .toBeInTheDocument();
  expect(await screen.findByText(/This Hive is now running 0.6.2/, {}, { timeout: 4000 }))
    .toBeInTheDocument();
}, 10000);

test("does not poll while no install is in flight", async () => {
  vi.mocked(api.fetchReleaseStatus).mockResolvedValue(status());
  render(<ReleaseUpdateAction busy={false} operatorToken="token" />);
  await waitFor(() => expect(api.fetchReleaseStatus).toHaveBeenCalledTimes(1));
  await new Promise((resolve) => setTimeout(resolve, 2500));
  expect(api.fetchReleaseStatus).toHaveBeenCalledTimes(1);
}, 10000);

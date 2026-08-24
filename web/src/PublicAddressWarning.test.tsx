import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import PublicAddressWarning from "./PublicAddressWarning";
import type { TunnelStatus } from "./api";

afterEach(cleanup);

const stopped: TunnelStatus = {
  available: true, running: false, serving: false, error: null,
  url: null, started_at: null, qr_svg: null,
};

test("says nothing while this Hive is only reachable locally", () => {
  render(<PublicAddressWarning status={stopped} onOpen={vi.fn()} />);
  expect(screen.queryByRole("button")).toBeNull();
  cleanup();
  render(<PublicAddressWarning status={undefined} onOpen={vi.fn()} />);
  expect(screen.queryByRole("button")).toBeNull();
});

test("names the exposure and the address once the Hive is on the internet", () => {
  // The operator accepted the exposure and refused the invisibility: it opens
  // the app to the web even for a Hive that was otherwise only on localhost,
  // and the only thing that said so was the settings card you had to be on.
  const onOpen = vi.fn();
  render(
    <PublicAddressWarning
      status={{ ...stopped, running: true, serving: true, url: "https://africa-sydney-prostores-behavior.trycloudflare.com" }}
      onOpen={onOpen}
    />,
  );
  const warning = screen.getByRole("button");
  expect(warning).toHaveTextContent("On the internet");
  expect(warning).toHaveTextContent("africa-sydney-prostores-behavior.trycloudflare.com");
  // The scheme is dropped for width; the address must still be recognisable.
  expect(warning).not.toHaveTextContent("https://");

  fireEvent.click(warning);
  expect(onOpen).toHaveBeenCalledTimes(1);
});

test("warns while the address is still being published, before it serves", () => {
  // running and not yet serving is still cloudflared holding a public name for
  // this Hive. Waiting for it to serve before saying anything would leave the
  // window nobody is told about.
  render(<PublicAddressWarning status={{ ...stopped, running: true, serving: false }} onOpen={vi.fn()} />);
  expect(screen.getByRole("button")).toHaveTextContent("Going on the internet");
});

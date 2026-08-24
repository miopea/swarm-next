import { useState } from "react";

import type { TunnelStatus } from "./api";

type Props = {
  status: TunnelStatus | undefined;
  onOpen: () => void;
  onStop: () => Promise<void>;
};

/**
 * Says, on every surface, that this Hive is reachable from the internet — and
 * ends it from there.
 *
 * A quick tunnel publishes the whole Hive, including one that was otherwise
 * only on localhost. The only thing that said so was the settings card an
 * operator had to already be on. The operator accepted the exposure and
 * refused the invisibility, and then asked for the obvious follow-on: the
 * place that tells you should also be the place that stops it.
 *
 * A state, not a destination, so it is styled as a warning rather than as
 * another item in the rail.
 */
export default function PublicAddressWarning({ status, onOpen, onStop }: Props) {
  const [stopping, setStopping] = useState(false);
  if (!status?.running) return null;
  const host = (status.url ?? "").replace(/^https:\/\//, "");
  return (
    <div className="public-address-warning" role="status">
      <button
        type="button"
        className="public-address-open"
        onClick={onOpen}
        aria-label={status.serving
          ? `This Hive is on the internet at ${host || "a temporary address"}. Open the sharing controls.`
          : "This Hive is being published to the internet. Open the sharing controls."}
      >
        <span className="public-address-pulse" aria-hidden="true" />
        <span>
          <strong>{status.serving ? "On the internet" : "Going on the internet"}</strong>
          <small>{status.serving
            ? host || "anyone with the address can reach this Hive"
            : "checking the address"}</small>
        </span>
      </button>
      <button
        type="button"
        className="public-address-stop"
        disabled={stopping}
        onClick={() => {
          setStopping(true);
          void onStop().finally(() => setStopping(false));
        }}
      >{stopping ? "Stopping…" : "Stop sharing"}</button>
    </div>
  );
}

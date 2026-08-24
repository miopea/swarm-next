import type { TunnelStatus } from "./api";

type Props = { status: TunnelStatus | undefined; onOpen: () => void };

/**
 * Says, on every surface, that this Hive is reachable from the internet.
 *
 * A quick tunnel publishes the whole Hive, and until now the only thing that
 * said so was the settings card an operator had to already be on. The operator
 * accepted the exposure and refused the invisibility: it should be obvious on
 * every page, because it opens the app to the web even for a Hive that was
 * otherwise only on localhost.
 *
 * Deliberately not shaped like the other rail destinations. It is a state, so
 * it reads as a warning first and as a way to the control that ends it second.
 */
export default function PublicAddressWarning({ status, onOpen }: Props) {
  if (!status?.running) return null;
  const host = (status.url ?? "").replace(/^https:\/\//, "");
  return (
    <button
      type="button"
      className="public-address-warning"
      onClick={onOpen}
      aria-label={status.serving
        ? `This Hive is on the internet at ${host || "a temporary address"}. Open the control that stops it.`
        : "This Hive is being published to the internet. Open the control that stops it."}
    >
      <span className="public-address-pulse" aria-hidden="true" />
      <span>
        <strong>{status.serving ? "On the internet" : "Going on the internet"}</strong>
        <small>{status.serving
          ? host || "anyone with the address can reach this Hive"
          : "checking the address"}</small>
      </span>
    </button>
  );
}

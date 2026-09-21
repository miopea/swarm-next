import type { ApiaryWatch } from "./api";

type Props = {
  watches: ApiaryWatch[] | undefined;
  /** Display name for an operator id, when the roster is known. */
  nameFor?: (operatorId: string) => string | undefined;
  onEnd?: (watchId: string) => Promise<void> | void;
};

/**
 * Says, on every surface, that someone is watching this Hive — and ends it from
 * here.
 *
 * ⚠️ THIS IS NOT DECORATION. Watching a Hive is a full live window into it, and
 * the operator's decision to allow that was made together with the promise that
 * it is never invisible. Without this notice the feature is standing
 * surveillance of someone's own machine, which is the thing the design says
 * must not ship. If this component stops rendering, the watch must stop being
 * granted.
 *
 * It names WHO, because that is what was decided, and it cannot say why: a
 * reason is deliberately not required of a watcher. Same placement and the same
 * argument as the public address warning — a state this Hive is in, not an
 * interruption.
 */
export default function WatchedByNotice({ watches, nameFor, onEnd }: Props) {
  // ⚠️ THIS COMPONENT FAILING IS AN INVISIBLE WATCH. Reading straight off the
  // body threw on anything unshaped and took the whole app down with it, which
  // is not merely a crash here — an operator being watched by a notice that
  // cannot render is the exact state this feature is forbidden to reach. So it
  // is defensive on purpose, and stays rendering whatever it can understand.
  const open = (Array.isArray(watches) ? watches : []).filter((watch) => Boolean(watch?.id) && watch.state !== "ended");
  if (open.length === 0) return null;
  const names = open.map((watch) => nameFor?.(watch.watcher_operator_id) ?? "Another operator");
  const pending = open.some((watch) => watch.state === "requested");
  return (
    <div className="watched-by-notice" role="status">
      <strong>{names.join(", ")} {open.length === 1 ? "is" : "are"} watching this Hive</strong>
      <small>{pending ? "Opening a live window. Nothing is relayed until this Hive confirms." : "A live window. Nothing is recorded."}</small>
      {onEnd ? <button type="button" onClick={() => void onEnd(open[0].id)}>Stop it</button> : null}
    </div>
  );
}

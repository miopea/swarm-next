import { useEffect, useState, type ComponentProps } from "react";
import { fetchSupportStatus, type SupportStatus } from "../api/support";
import { useModalFocus } from "../shared/useModalFocus";
import DogfoodFeedbackDialog from "./DogfoodFeedbackDialog";
import SupportFeedbackDialog from "./SupportFeedbackDialog";
import { loadPendingSupport } from "./supportDraft";

/** Destination is established before showing a form; errors do not silently choose a public channel. */
export default function FeedbackDialog(props: ComponentProps<typeof DogfoodFeedbackDialog>) {
  const [hasPending] = useState(() => { try { return Boolean(loadPendingSupport()); } catch { return true; } });
  const [status, setStatus] = useState<SupportStatus>();
  const [error, setError] = useState(false);
  const [revision, setRevision] = useState(0);
  const modal = useModalFocus<HTMLElement>(props.onClose, !status);
  useEffect(() => {
    let current = true;
    const controller = new AbortController();
    const deadline = window.setTimeout(() => controller.abort(), 8_000);
    setError(false);
    void fetchSupportStatus(props.operatorToken, controller.signal)
      .then((next) => { if (current) setStatus(next); })
      .catch(() => { if (current) setError(true); })
      .finally(() => window.clearTimeout(deadline));
    return () => { current = false; controller.abort(); window.clearTimeout(deadline); };
  }, [props.operatorToken, revision]);
  if (status && (status.configured || status.deliveries.length > 0 || hasPending)) return <SupportFeedbackDialog operatorToken={props.operatorToken} status={status} onClose={props.onClose} onSaved={props.onSaved} />;
  if (status) return <DogfoodFeedbackDialog {...props} />;
  return <div className="feedback-backdrop"><section ref={modal} tabIndex={-1} className="feedback-dialog" role="dialog" aria-modal="true" aria-label="Feedback destination">
    <h2>Feedback</h2><p role={error ? "alert" : "status"}>{error ? "Unable to check where feedback will go. Nothing has been sent." : "Checking the feedback destination…"}</p>
    {error && <button className="secondary-button" onClick={() => setRevision((value) => value + 1)}>Retry</button>}
    <button className="secondary-button" onClick={props.onClose}>Close</button>
  </section></div>;
}

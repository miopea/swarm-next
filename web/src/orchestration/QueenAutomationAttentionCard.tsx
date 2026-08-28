import { useState } from "react";

import type { QueenAutomationStatus } from "../api";
import BeeMascot from "../brand/BeeMascot";
import {
  queenAutomationNeedsAttention,
  queenAutomationStateDetail,
  queenAutomationStateLabel,
} from "./queenAutomationPresentation";

type Props = {
  status: QueenAutomationStatus | undefined;
  coveredBySpecificDecision?: boolean;
  /** Whether one of Queen's own requests is actually waiting to be answered. */
  queenRequestPending?: boolean;
  onOpenQueen: () => void;
  onReviewSettings: () => void;
  /** Resumes the review. Absent when there is nothing to resume. */
  onRetry?: () => Promise<void>;
};

export default function QueenAutomationAttentionCard({ status, coveredBySpecificDecision = false, queenRequestPending = false, onOpenQueen, onReviewSettings, onRetry }: Props) {
  const [retrying, setRetrying] = useState(false);
  const [failed, setFailed] = useState(false);
  if (!queenAutomationNeedsAttention(status, queenRequestPending) || coveredBySpecificDecision) return null;
  // The card said what was wrong in three places and offered no way to end it;
  // the only control that resolved this lived in settings. Opening Queen stays
  // first, because the message asks the operator to check her terminal before
  // resuming, but the resuming itself is now reachable from here.
  const canRetry = Boolean(onRetry) && status?.state === "uncertain";
  return (
    <section className="queen-attention-card" aria-labelledby="queen-attention-heading">
      <span className="queen-attention-bee" aria-hidden="true"><BeeMascot role="queen" expression="blocked" /></span>
      <div>
        <p className="eyebrow">Queen automation</p>
        <h3 id="queen-attention-heading">{queenAutomationStateLabel(status)}</h3>
        <p>{queenAutomationStateDetail(status, "attention")}</p>
      </div>
      <div className="queen-attention-actions">
        <button className="primary-action" type="button" onClick={onOpenQueen}>Open Queen</button>
        {canRetry ? (
          <button
            className="secondary-button"
            type="button"
            disabled={retrying}
            onClick={async () => {
              setRetrying(true);
              setFailed(false);
              try {
                await onRetry?.();
              } catch {
                setFailed(true);
              } finally {
                setRetrying(false);
              }
            }}
          >{retrying ? "Resuming…" : "Resume review"}</button>
        ) : null}
        <button className="secondary-button" type="button" onClick={onReviewSettings}>Review automation</button>
        {failed ? <small role="alert">Queen could not resume the review. Her current work was not changed.</small> : null}
      </div>
    </section>
  );
}

import type { QueenAutomationStatus } from "../api";
import BeeMascot from "../brand/BeeMascot";
import {
  queenAutomationNeedsAttention,
  queenAutomationStateDetail,
  queenAutomationStateLabel,
} from "./queenAutomationPresentation";

type Props = {
  status: QueenAutomationStatus | undefined;
  onOpenQueen: () => void;
  onReviewSettings: () => void;
};

export default function QueenAutomationAttentionCard({ status, onOpenQueen, onReviewSettings }: Props) {
  if (!queenAutomationNeedsAttention(status)) return null;
  return (
    <section className="queen-attention-card" aria-labelledby="queen-attention-heading">
      <span className="queen-attention-bee" aria-hidden="true"><BeeMascot expression="blocked" /></span>
      <div>
        <p className="eyebrow">Queen automation</p>
        <h3 id="queen-attention-heading">{queenAutomationStateLabel(status)}</h3>
        <p>{queenAutomationStateDetail(status)}</p>
      </div>
      <div className="queen-attention-actions">
        <button className="primary-action" type="button" onClick={onOpenQueen}>Open Queen</button>
        <button className="secondary-button" type="button" onClick={onReviewSettings}>Review automation</button>
      </div>
    </section>
  );
}

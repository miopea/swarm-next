type Props = {
  pendingAssistance: number;
  onReview: () => void;
};

export default function ApiaryAttentionCard({ pendingAssistance, onReview }: Props) {
  if (!pendingAssistance) return null;
  return (
    <section className="apiary-attention-card" aria-labelledby="apiary-attention-heading">
      <div>
        <p className="eyebrow">Apiary assistance</p>
        <h3 id="apiary-attention-heading">A trusted Steward offered help</h3>
        <p>{pendingAssistance === 1 ? "One offer is" : `${pendingAssistance} offers are`} waiting for you to accept or decline. Nothing was sent to a worker or terminal.</p>
      </div>
      <button className="primary-action" type="button" onClick={onReview}>Review in Apiary</button>
    </section>
  );
}

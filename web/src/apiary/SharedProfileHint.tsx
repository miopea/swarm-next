/** Old Hives retain their saved profile until their operator reviews a change. */
export default function SharedProfileHint({ name, onReview }: { name: string; onReview: () => void }) {
  if (name.trim() && name.trim() !== "Operator") return null;
  return <section className="apiary-profile-hint" aria-label="Complete your shared profile">
    <div><strong>Help your Apiary recognize you</strong><p>Your shared name is still unset. Review details from Jira or Microsoft, or enter them once. Your saved profile is also reused for feedback.</p></div>
    <button type="button" className="secondary-button" onClick={onReview}>Review shared profile</button>
  </section>;
}

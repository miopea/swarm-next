/** This warning cannot depend on a decision stored in the damaged database. */
export default function DatabaseRecoveryCard() {
  return <article className="decision-card urgency-time_sensitive" role="alert">
    <p className="eyebrow">Database recovery required</p>
    <h3>Swarm needs a verified database restore</h3>
    <p>Database integrity verification failed. New database-backed work and coordination are paused. Running worker processes have not been stopped.</p>
    <details className="decision-argument">
      <summary>Recovery details</summary>
      <p>Use the installation’s <code>restore-offline</code> command with a verified backup. It preserves the damaged database for inspection and restarts only the API. Do not keep retrying task changes or replace the database while the API is running.</p>
      <p>This notice clears after the restored database passes verification and the API reopens it.</p>
    </details>
  </article>;
}

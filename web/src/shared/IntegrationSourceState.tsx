export default function IntegrationSourceState({
  source,
  title,
  detail,
  action,
  onAction,
  className = "",
}: {
  source: string;
  title: string;
  detail: string;
  action?: string;
  onAction?: () => void;
  className?: string;
}) {
  return <section className={className} aria-label={`${source} work status`}>
    <div><p className="eyebrow">{source} work</p><h3>{title}</h3></div>
    <p>{detail}</p>
    {action && onAction ? <button type="button" className="secondary-button" onClick={onAction}>{action}</button> : null}
  </section>;
}

import { useEffect, useMemo, useState } from "react";

export type CommandChoice = { id: string; label: string; detail: string; group: "Go to" | "Workers" | "Work" | "Attention"; run: () => void };

export default function CommandPalette({ choices, onClose }: { choices: CommandChoice[]; onClose: () => void }) {
  const [query, setQuery] = useState("");
  const filtered = useMemo(() => {
    const needle = query.trim().toLocaleLowerCase();
    return needle ? choices.filter((choice) => `${choice.label} ${choice.detail}`.toLocaleLowerCase().includes(needle)) : choices;
  }, [choices, query]);
  useEffect(() => {
    const close = (event: KeyboardEvent) => { if (event.key === "Escape") onClose(); };
    window.addEventListener("keydown", close);
    return () => window.removeEventListener("keydown", close);
  }, [onClose]);
  return <div className="command-backdrop" role="presentation" onMouseDown={(event) => { if (event.target === event.currentTarget) onClose(); }}>
    <section className="command-palette" role="dialog" aria-modal="true" aria-labelledby="command-heading">
      <header className="command-header">
        <div><p className="eyebrow">Quick navigation</p><h2 id="command-heading">Where would you like to go?</h2></div>
        <button type="button" className="secondary-button" onClick={onClose}>Close</button>
      </header>
      <label className="sr-only" htmlFor="command-query">Find a view or worker</label>
      <input id="command-query" autoFocus value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Find a view or worker…" />
      <div className="command-results">
        {filtered.map((choice) => <button key={choice.id} type="button" onClick={() => { onClose(); choice.run(); }}>
          <span><small>{choice.group}</small><strong>{choice.label}</strong></span><span>{choice.detail}</span>
        </button>)}
        {filtered.length === 0 ? <p>No matching view or worker.</p> : null}
      </div>
      <small className="privacy-note">Tip: press Alt+K anywhere outside a terminal or text field. Sleeping workers wake when selected.</small>
    </section>
  </div>;
}

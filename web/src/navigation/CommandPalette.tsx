import { useEffect, useMemo, useState } from "react";

export type CommandChoice = { id: string; label: string; detail: string; group: "Go to" | "Workers" | "Work" | "Attention"; run: () => void };

export default function CommandPalette({ choices, onClose }: { choices: CommandChoice[]; onClose: () => void }) {
  const [query, setQuery] = useState("");
  const [activeIndex, setActiveIndex] = useState(0);
  const filtered = useMemo(() => {
    const needle = query.trim().toLocaleLowerCase();
    return needle ? choices.filter((choice) => `${choice.label} ${choice.detail}`.toLocaleLowerCase().includes(needle)) : choices;
  }, [choices, query]);
  useEffect(() => setActiveIndex(0), [query]);
  useEffect(() => {
    setActiveIndex((current) => Math.min(current, Math.max(filtered.length - 1, 0)));
  }, [filtered.length]);
  useEffect(() => {
    const choice = filtered[activeIndex];
    if (choice) document.getElementById(`command-${choice.id}`)?.scrollIntoView?.({ block: "nearest" });
  }, [activeIndex, filtered]);
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
      <label className="sr-only" htmlFor="command-query">Find work, decisions, or workers</label>
      <input
        id="command-query"
        autoFocus
        value={query}
        onChange={(event) => setQuery(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "ArrowDown" && filtered.length) {
            event.preventDefault();
            setActiveIndex((current) => (current + 1) % filtered.length);
          } else if (event.key === "ArrowUp" && filtered.length) {
            event.preventDefault();
            setActiveIndex((current) => (current - 1 + filtered.length) % filtered.length);
          } else if (event.key === "Enter" && filtered[activeIndex]) {
            event.preventDefault();
            onClose();
            filtered[activeIndex].run();
          }
        }}
        placeholder="Find work, decisions, or workers…"
        role="combobox"
        aria-autocomplete="list"
        aria-controls="command-results"
        aria-activedescendant={filtered[activeIndex] ? `command-${filtered[activeIndex].id}` : undefined}
      />
      <div className="command-results" id="command-results" role="listbox">
        {filtered.map((choice, index) => <button id={`command-${choice.id}`} aria-selected={index === activeIndex} role="option" key={choice.id} type="button" onMouseEnter={() => setActiveIndex(index)} onClick={() => { onClose(); choice.run(); }}>
          <span><small>{choice.group}</small><strong>{choice.label}</strong></span><span>{choice.detail}</span>
        </button>)}
        {filtered.length === 0 ? <p>No matching result.</p> : null}
      </div>
      <small className="privacy-note">Tip: press Alt+K anywhere outside a terminal or text field. Sleeping workers wake when selected.</small>
    </section>
  </div>;
}

import { useEffect, useMemo, useRef, useState } from "react";
import { useModalFocus } from "../shared/useModalFocus";
import ModalPortal from "../shared/ModalPortal";

export type CommandChoice = { id: string; label: string; detail: string; group: "Go to" | "Workers" | "Work" | "Attention"; run: () => void };

const PAGE_SIZE = 40;
function preview(text: string) {
  let result = "";
  let count = 0;
  for (const point of text ?? "") {
    if (count++ === 160) return `${result}…`;
    result += point;
  }
  return result;
}

export default function CommandPalette({ choices, onClose }: { choices: CommandChoice[]; onClose: () => void }) {
  const search = useRef<HTMLInputElement>(null);
  const dialog = useModalFocus<HTMLElement>(onClose, true, search);
  const [query, setQuery] = useState("");
  const [activeIndex, setActiveIndex] = useState(0);
  const searchable = useMemo(() => choices.map(choice => ({ choice, text: `${choice.label} ${choice.detail}`.toLocaleLowerCase() })), [choices]);
  const filtered = useMemo(() => {
    const needle = query.trim().toLocaleLowerCase();
    return needle ? searchable.filter(item => item.text.includes(needle)).map(item => item.choice) : choices;
  }, [choices, searchable, query]);
  const selectedIndex = Math.min(activeIndex, Math.max(filtered.length - 1, 0));
  const pageStart = Math.floor(selectedIndex / PAGE_SIZE) * PAGE_SIZE;
  const visible = filtered.slice(pageStart, pageStart + PAGE_SIZE);
  useEffect(() => setActiveIndex(0), [query]);
  useEffect(() => {
    setActiveIndex((current) => Math.min(current, Math.max(filtered.length - 1, 0)));
  }, [filtered.length]);
  useEffect(() => {
    const choice = filtered[activeIndex];
    if (choice) document.getElementById(`command-${choice.id}`)?.scrollIntoView?.({ block: "nearest" });
  }, [activeIndex, filtered]);
  return <ModalPortal><div className="command-backdrop" role="presentation" onMouseDown={(event) => { if (event.target === event.currentTarget) onClose(); }}>
    <section ref={dialog} tabIndex={-1} className="command-palette" role="dialog" aria-modal="true" aria-labelledby="command-heading">
      <header className="command-header">
        <div><p className="eyebrow">Quick navigation</p><h2 id="command-heading">Where would you like to go?</h2></div>
        <button type="button" className="secondary-button" onClick={onClose}>Close</button>
      </header>
      <label className="sr-only" htmlFor="command-query">Find work, decisions, or workers</label>
      <input
        id="command-query"
        ref={search}
        value={query}
        onChange={(event) => setQuery(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "ArrowDown" && filtered.length) {
            event.preventDefault();
            setActiveIndex((current) => (current + 1) % filtered.length);
          } else if (event.key === "ArrowUp" && filtered.length) {
            event.preventDefault();
            setActiveIndex((current) => (current - 1 + filtered.length) % filtered.length);
          } else if (event.key === "Enter" && filtered[selectedIndex]) {
            event.preventDefault();
            onClose();
            filtered[selectedIndex].run();
          }
        }}
        placeholder="Find work, decisions, or workers…"
        role="combobox"
        aria-expanded="true"
        aria-autocomplete="list"
        aria-controls="command-results"
        aria-activedescendant={filtered[selectedIndex] ? `command-${filtered[selectedIndex].id}` : undefined}
      />
      <div className="command-results" id="command-results" role="listbox">
        {visible.map((choice, index) => <button id={`command-${choice.id}`} data-group={choice.group} aria-selected={pageStart + index === selectedIndex} role="option" aria-posinset={pageStart + index + 1} aria-setsize={filtered.length} key={choice.id} type="button" onMouseEnter={() => setActiveIndex(pageStart + index)} onClick={() => { onClose(); choice.run(); }}>
          <span><small>{choice.group}</small><strong>{preview(choice.label)}</strong></span><span>{preview(choice.detail)}</span>
        </button>)}
        {filtered.length === 0 ? <p>No matching result.</p> : null}
      </div>
      <footer>
        {filtered.length > PAGE_SIZE && <nav aria-label="Search result pages" className="command-pages">
          <button type="button" className="secondary-button" disabled={pageStart === 0} onClick={() => setActiveIndex(Math.max(0, pageStart - PAGE_SIZE))}>Previous results</button>
          <span role="status">{pageStart + 1}–{pageStart + visible.length} of {filtered.length}</span>
          <button type="button" className="secondary-button" disabled={pageStart + PAGE_SIZE >= filtered.length} onClick={() => setActiveIndex(pageStart + PAGE_SIZE)}>Next results</button>
        </nav>}
        <small className="privacy-note">Tip: press Alt+K anywhere outside a terminal or text field. Sleeping workers wake when selected.</small>
      </footer>
    </section>
  </div></ModalPortal>;
}

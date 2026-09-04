export const MAX_TERMINAL_DRAFT_LENGTH = 16_384;
const KEY = "swarm.terminal-draft.v1";
type Draft = { sessionId: string; text: string; uncertain: boolean };
type State = { draft?: Draft; storageUnavailable: boolean };

/** One tab-owned draft, never a per-worker cache or an automatic delivery queue. */
export class TerminalDraftStore {
  #state: State = { storageUnavailable: false };
  #loaded = false;
  #listeners = new Set<() => void>();
  constructor(private readonly storage: () => Pick<Storage, "getItem" | "setItem" | "removeItem"> = () => window.sessionStorage) {}
  snapshot = (): State => {
    if (!this.#loaded) {
      this.#loaded = true;
      try {
        const raw = this.storage().getItem(KEY);
        if (raw && raw.length <= MAX_TERMINAL_DRAFT_LENGTH * 6 + 1_024) {
          const value = JSON.parse(raw);
          if (value?.schema === 1 && typeof value.sessionId === "string" && value.sessionId.length > 0 && value.sessionId.length <= 256
            && typeof value.text === "string" && value.text.length > 0 && value.text.length <= MAX_TERMINAL_DRAFT_LENGTH && typeof value.uncertain === "boolean") {
            this.#state = { storageUnavailable: false, draft: { sessionId: value.sessionId, text: value.text, uncertain: value.uncertain } };
          }
        }
      } catch { this.#state = { ...this.#state, storageUnavailable: true }; }
    }
    return this.#state;
  };
  subscribe = (listener: () => void) => {
    if (this.#listeners.size >= 32) throw new Error("Terminal draft subscriber limit reached");
    this.#listeners.add(listener);
    return () => { this.#listeners.delete(listener); };
  };
  #publish(state: State) { this.#state = state; for (const listener of this.#listeners) listener(); }
  update(sessionId: string, text: string): boolean {
    const state = this.snapshot();
    if (!sessionId || sessionId.length > 256 || text.length > MAX_TERMINAL_DRAFT_LENGTH || (state.draft && state.draft.sessionId !== sessionId)) return false;
    this.#publish({ ...state, draft: text ? { sessionId, text, uncertain: state.draft?.uncertain ?? false } : undefined });
    if (!text) this.flush();
    return true;
  }
  markUncertain(sessionId: string, uncertain: boolean) {
    const state = this.snapshot();
    if (state.draft?.sessionId !== sessionId) return;
    this.#publish({ ...state, draft: { ...state.draft, uncertain } });
    this.flush();
  }
  clear() {
    this.#loaded = true;
    this.#publish({ storageUnavailable: this.#state.storageUnavailable });
    this.flush();
  }
  flush = () => {
    const state = this.snapshot();
    let storageUnavailable = false;
    try {
      if (state.draft) this.storage().setItem(KEY, JSON.stringify({ schema: 1, ...state.draft }));
      else this.storage().removeItem(KEY);
    } catch { storageUnavailable = true; }
    if (state.storageUnavailable !== storageUnavailable) this.#publish({ ...state, storageUnavailable });
  };
}

export const terminalDraft = new TerminalDraftStore();

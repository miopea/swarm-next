import WatchWindow, { type WatchSurface } from "../apiary/WatchWindow";

/**
 * The watch window at full size, with no Hive and no stream.
 *
 * ⚠️ THIS EXISTS BECAUSE THE WATCH UI SHIPPED WITHOUT ANYONE LOOKING AT IT. The
 * operator's verdict on 2026-09-22 was "the UI for watching is terrible": a
 * panel a few hundred pixels wide inside the Hive roster, somebody else's
 * terminal reflowed into it, and no control but "Close window". The surface is
 * supplied here so nothing opens a socket — what is being checked is the frame,
 * the tools and how much room the terminal gets.
 */
export default function ApiaryWatchFixture() {
  return <WatchWindow
    watchId="fictional"
    operatorToken="fictional"
    hiveName="Clover Hive"
    onClose={() => {}}
    onTakeOver={() => {}}
    createSurface={(host) => {
      const pre = document.createElement("pre");
      pre.className = "xterm-screen";
      pre.style.cssText = "margin:0;color:#d9e7d4;font:12px/1.4 monospace;white-space:pre;width:640px;height:340px";
      pre.textContent = [
        "> Tell me the time",
        "  Bash(date)",
        "  └ Tue Sep 22 11:32:59 EDT 2026",
        "",
        "  11:32 AM EDT, Tuesday, September 22, 2026.",
        "",
        "> Testing",
        "  Got it — I'm here. What do you need?",
        "",
        "  auto mode on (shift+tab to cycle) · ← for agents",
      ].join("\n");
      host.appendChild(pre);
      return {
        write: () => {},
        resize: () => {},
        clear: () => { pre.textContent = ""; },
        dispose: () => pre.remove(),
      } satisfies WatchSurface;
    }}
  />;
}

import { Terminal } from "@xterm/xterm";

import WatchWindow, { type WatchSurface } from "../apiary/WatchWindow";
import { mirrorFont, mirrorTerminalOptions } from "../apiary/mirrorScale";

/**
 * The watch window at full size, with no Hive and no stream.
 *
 * ⚠️ THIS EXISTS BECAUSE THE WATCH UI SHIPPED WITHOUT ANYONE LOOKING AT IT. The
 * operator's verdict on 2026-09-22 was "the UI for watching is terrible": a
 * panel a few hundred pixels wide inside the Hive roster, somebody else's
 * terminal reflowed into it, and no control but "Close window". The surface is
 * supplied here so nothing is streamed — what is being checked is the frame,
 * the tools, the typeface, and how much room the terminal gets.
 */
export default function ApiaryWatchFixture() {
  return <WatchWindow
    watchId="fictional"
    operatorToken="fictional"
    hiveName="Clover Hive"
    onClose={() => {}}
    onTakeOver={() => {}}
    createSurface={(host) => {
      // A real terminal at the size the WSL Hive's Queen ran at, so the font
      // and the fit are the ones the window really draws.
      const terminal = new Terminal({ ...mirrorTerminalOptions(), disableStdin: true, cursorBlink: false, cols: 80, rows: 24 });
      terminal.open(host);
      terminal.write([
        "> Tell me the time",
        "",
        "  Bash(date)",
        "  └ Tue Sep 22 11:32:59 EDT 2026",
        "",
        "  11:32 AM EDT, Tuesday, September 22, 2026.",
        "",
        "> Testing",
        "",
        "  Got it — I'm here. What do you need?",
        "",
        "  auto mode on (shift+tab to cycle) · ← for agents",
      ].join("\r\n"));
      return {
        write: () => {},
        resize: () => {},
        clear: () => terminal.reset(),
        dispose: () => terminal.dispose(),
        font: mirrorFont(terminal),
      } satisfies WatchSurface;
    }}
  />;
}

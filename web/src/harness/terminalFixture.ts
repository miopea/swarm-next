/**
 * A terminal that plays a session which never happened.
 *
 * WHY THIS EXISTS, and why it is the only safe way to photograph a terminal.
 * The Workers screen is mostly an xterm CANVAS. Canvas contents cannot be
 * scrubbed the way the DOM can — there is no node to rewrite and no regex that
 * reaches pixels — so the anonymise-then-capture approach used for the 0.9.2
 * marketing captures does not apply here at all. Real sessions inspected during
 * that work held the operator's banking institutions and another product's
 * password-policy internals. The rule that follows is absolute: never publish a
 * capture of an actual worker session.
 *
 * So rather than scrubbing a real one, this invents one. Every character below
 * is fiction, written to read like real work on the fictional Orchard Hive the
 * rest of these fixtures use. Safe by construction, not by inspection.
 *
 * WHY IT REPLACES WebSocket RATHER THAN THREADING A PROP. TerminalConnection
 * does accept a websocketFactory, but it is constructed deep inside the
 * component tree, so reaching it from here means a prop through every layer in
 * between — and every socket added below it later escapes the seam. The harness
 * already learned this with fetch and stubs the NETWORK instead. A socket is
 * network. It gets the same treatment at the same boundary, and nothing in
 * production has to learn that a harness exists.
 */

const SNAPSHOT_FRAME_TYPE = 2;

/** Built from a char code so no raw escape byte sits in this source file. */
const CSI = `${String.fromCharCode(27)}[`;
/** Dim, for paths and chrome. */
const D = `${CSI}38;5;245m`;
/** The agent's bullet. */
const B = `${CSI}38;5;110m`;
const GREEN = `${CSI}38;5;71m`;
const AMBER = `${CSI}38;5;179m`;
const BOLD = `${CSI}1m`;
const R = `${CSI}0m`;

/**
 * INVENTED. A worker on the fictional Orchard Web repository, working a bug
 * specific enough to read as real work and belonging to no actual product.
 */
const TRANSCRIPT: string = [
  `${D}you@orchard${R}:${D}~/projects/orchard-web${R}$ claude`,
  ``,
  `  ${BOLD}Orchard Web${R} ${D}· worker · claude code${R}`,
  ``,
  `${D}>${R} Signed-out visitors hit a redirect loop on /account. They`,
  `  should land on the pricing page instead.`,
  ``,
  `${B}●${R} Following the whole redirect chain before changing anything,`,
  `  because a loop is never in the hop you are looking at.`,
  ``,
  `  ${D}Read${R}  src/routes/account.tsx ${D}(140 lines)${R}`,
  `  ${D}Read${R}  src/auth/guard.ts ${D}(72 lines)${R}`,
  `  ${D}Grep${R}  "redirect(" ${D}→ 9 matches in 5 files${R}`,
  ``,
  `${B}●${R} Two rules, each correct on its own.`,
  ``,
  `  The guard sends a signed-out visitor to /signin. /signin sends`,
  `  anyone without a plan to /account. Neither knows the other`,
  `  exists, so the pair hands the visitor back and forth forever.`,
  ``,
  `  ${D}src/auth/guard.ts:34${R}`,
  `    return redirect("/signin");`,
  ``,
  `  ${D}Edit${R}  src/auth/guard.ts`,
  `  ${D}Test${R}  npm test -- auth`,
  ``,
  `  ${GREEN}✓${R} a signed-out visitor lands on /pricing`,
  `  ${GREEN}✓${R} a signed-in visitor without a plan still reaches /signin`,
  `  ${D}28 passed in 3.4s${R}`,
  ``,
  `${B}●${R} The second test is the one worth having. Sending everyone to`,
  `  /pricing would have fixed the loop and stranded members who had`,
  `  simply not renewed yet.`,
  ``,
  `  ${AMBER}Not verified:${R} I have not exercised this against a real`,
  `  session cookie, so the redirect is covered by unit tests only.`,
  ``,
  `${D}>${R} `,
].join("\r\n");

function encodeSnapshotFrame(text: string, rows: number, columns: number): ArrayBuffer {
  const payload = new TextEncoder().encode(text);
  // 1 type + 8 sequence + 2 rows + 2 columns + 1 truncated, then the bytes.
  const frame = new Uint8Array(14 + payload.byteLength);
  frame[0] = SNAPSHOT_FRAME_TYPE;
  new DataView(frame.buffer, 1, 8).setBigUint64(0, 1n);
  const dimensions = new DataView(frame.buffer, 9, 4);
  dimensions.setUint16(0, rows);
  dimensions.setUint16(2, columns);
  frame[13] = 0; // not truncated
  frame.set(payload, 14);
  return frame.buffer;
}

/**
 * Enough of a WebSocket for TerminalConnection, and nothing more.
 *
 * It answers the client's `resume` with a canonical snapshot followed by a
 * running state, which is the same sequence a real attachment produces. It
 * never reaches the network, and it accepts keystrokes without echoing them:
 * the picture is of a session already in progress, not an interactive one.
 */
export class FixtureWebSocket extends EventTarget {
  static readonly CONNECTING = 0;
  static readonly OPEN = 1;
  static readonly CLOSING = 2;
  static readonly CLOSED = 3;

  readonly url: string;
  readonly protocol: string;
  binaryType: "blob" | "arraybuffer" = "blob";
  readyState: number = FixtureWebSocket.CONNECTING;

  constructor(url: string, protocols: string[] = []) {
    super();
    this.url = url;
    this.protocol = protocols[0] ?? "";
    queueMicrotask(() => {
      this.readyState = FixtureWebSocket.OPEN;
      this.dispatchEvent(new Event("open"));
    });
  }

  send(data: unknown): void {
    if (typeof data !== "string") return;
    let message: { type?: string; rows?: number; columns?: number };
    try {
      message = JSON.parse(data) as typeof message;
    } catch {
      return;
    }
    if (message.type !== "resume") return;
    // Draw at whatever geometry the renderer actually asked for, so the fixture
    // never wedges the terminal at a width the viewport does not have.
    const rows = message.rows && message.rows > 0 ? message.rows : 32;
    const columns = message.columns && message.columns > 0 ? message.columns : 120;
    queueMicrotask(() => {
      this.dispatchEvent(
        new MessageEvent("message", { data: encodeSnapshotFrame(TRANSCRIPT, rows, columns) }),
      );
      this.dispatchEvent(
        new MessageEvent("message", {
          data: JSON.stringify({
            type: "state",
            running: true,
            latest_sequence: 1,
            geometry_owned: true,
          }),
        }),
      );
    });
  }

  close(): void {
    this.readyState = FixtureWebSocket.CLOSED;
    this.dispatchEvent(new CloseEvent("close"));
  }
}

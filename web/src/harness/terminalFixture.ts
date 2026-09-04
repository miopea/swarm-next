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

function encodeSnapshotFrame(text: string, rows: number, columns: number, sequence: number): ArrayBuffer {
  const payload = new TextEncoder().encode(text);
  // 1 type + 8 sequence + 2 rows + 2 columns + 1 truncated, then the bytes.
  const frame = new Uint8Array(14 + payload.byteLength);
  frame[0] = SNAPSHOT_FRAME_TYPE;
  new DataView(frame.buffer, 1, 8).setBigUint64(0, BigInt(sequence));
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
  #resumed = false;
  #owned: boolean;
  #occupied = true;
  #generation = 1;
  #sequence = 1;
  #rows = 32;
  #columns = 120;

  constructor(url: string, protocols: string[] = [], initiallyOwned = true) {
    super();
    this.url = url;
    this.protocol = protocols[0] ?? "";
    this.#owned = initiallyOwned;
    queueMicrotask(() => {
      if (this.readyState === FixtureWebSocket.CLOSED) return;
      this.readyState = FixtureWebSocket.OPEN;
      this.dispatchEvent(new Event("open"));
    });
  }

  send(data: unknown): void {
    if (typeof data !== "string") return;
    if (this.readyState !== FixtureWebSocket.OPEN) return;
    let message: { type?: string; rows?: number; columns?: number; request_id?: string; generation?: string; observed_generation?: string | null };
    try {
      message = JSON.parse(data) as typeof message;
    } catch {
      return;
    }
    if (message.type === "probe") {
      this.#reply({ type: "alive", request_id: message.request_id });
      return;
    }
    if (message.type === "claim") {
      if (message.observed_generation !== String(this.#generation) && !(message.observed_generation === null && (this.#owned || !this.#occupied))) {
        this.#reply({ type: "error", code: "terminal_control_owned_elsewhere", message: "Another synthetic view controls this fixture." });
        this.#publishControl();
        return;
      }
      if (!this.#owned) this.#generation++;
      this.#owned = true;
      this.#occupied = true;
      this.#resize(message.rows, message.columns);
      this.#publishControl();
      return;
    }
    if (["resize", "input", "renew", "release"].includes(message.type ?? "")) {
      if (!this.#owned || message.generation !== String(this.#generation)) {
        this.#reply({ type: "error", code: "terminal_control_stale", message: "Synthetic control is stale." });
        this.#publishControl();
        return;
      }
      if (message.type === "resize") this.#resize(message.rows, message.columns);
      if (message.type === "release") {
        this.#owned = false;
        this.#occupied = false;
        this.#generation++;
      }
      this.#publishControl();
      return;
    }
    if (message.type !== "resume") return;
    if (this.#resumed) {
      this.#reply({ type: "error", code: "duplicate_resume", message: "One resume per fixture socket." });
      return;
    }
    this.#resumed = true;
    // Draw at whatever geometry the renderer actually asked for, so the fixture
    // never wedges the terminal at a width the viewport does not have.
    const rows = message.rows && message.rows > 0 ? message.rows : 32;
    const columns = message.columns && message.columns > 0 ? message.columns : 120;
    this.#rows = rows;
    this.#columns = columns;
    queueMicrotask(() => {
      if (this.readyState !== FixtureWebSocket.OPEN) return;
      this.dispatchEvent(
        new MessageEvent("message", { data: encodeSnapshotFrame(TRANSCRIPT, rows, columns, this.#sequence) }),
      );
      this.dispatchEvent(
        new MessageEvent("message", {
          data: JSON.stringify({
            type: "state",
            running: true,
            latest_sequence: 1,
            control: this.#control(),
          }),
        }),
      );
    });
  }

  /** Disposable performance fixture only; never writes to a network socket. */
  emitOutput(bytes: Uint8Array): void {
    if (this.readyState !== FixtureWebSocket.OPEN || !this.#resumed) throw new Error("Fixture is not attached");
    if (bytes.byteLength > 65_536) throw new Error("Fixture packet exceeds its bound");
    const frame = new Uint8Array(9 + bytes.byteLength);
    frame[0] = 1;
    new DataView(frame.buffer).setBigUint64(1, BigInt(++this.#sequence));
    frame.set(bytes, 9);
    this.dispatchEvent(new MessageEvent("message", { data: frame.buffer }));
  }

  #control() {
    return { supported: true, generation: String(this.#generation), owned: this.#owned, occupied: this.#occupied, lease_remaining_ms: this.#occupied ? 90_000 : 0 };
  }

  #publishControl(): void { this.#reply({ type: "control", control: this.#control() }); }

  #reply(message: object): void {
    queueMicrotask(() => {
      if (this.readyState === FixtureWebSocket.OPEN) this.dispatchEvent(new MessageEvent("message", { data: JSON.stringify(message) }));
    });
  }

  #resize(rows?: number, columns?: number): void {
    if (!rows || !columns || rows <= 0 || columns <= 0 || (rows === this.#rows && columns === this.#columns)) return;
    this.#rows = rows;
    this.#columns = columns;
    const frame = encodeSnapshotFrame(TRANSCRIPT, rows, columns, ++this.#sequence);
    queueMicrotask(() => {
      if (this.readyState === FixtureWebSocket.OPEN) this.dispatchEvent(new MessageEvent("message", { data: frame }));
    });
  }

  close(): void {
    this.readyState = FixtureWebSocket.CLOSED;
    this.dispatchEvent(new CloseEvent("close"));
  }
}

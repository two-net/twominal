export type OscEvent =
  | { type: "idle"; cwd: string }
  | { type: "alt"; on: boolean }
  | { type: "appCursor"; on: boolean };

/**
 * Filters private Twominal OSC from the PTY stream and notices alt-screen /
 * application-cursor sequences. CSI is always forwarded so xterm can render.
 */
export class StreamFilter {
  private carry = "";
  private decoder = new TextDecoder("utf-8", { fatal: false });

  push(chunk: Uint8Array): { text: string; events: OscEvent[] } {
    const incoming = this.decoder.decode(chunk, { stream: true });
    let s = this.carry + incoming;
    this.carry = "";
    const events: OscEvent[] = [];
    let out = "";
    let i = 0;
    while (i < s.length) {
      if (s.charCodeAt(i) !== 0x1b) {
        const next = s.indexOf("\x1b", i);
        if (next < 0) {
          out += s.slice(i);
          i = s.length;
          break;
        }
        out += s.slice(i, next);
        i = next;
        continue;
      }
      if (i + 1 >= s.length) {
        this.carry = s.slice(i);
        break;
      }
      const kind = s[i + 1];
      if (kind === "]") {
        const parsed = consumeOsc(s, i);
        if (parsed.incomplete) {
          this.carry = s.slice(i);
          break;
        }
        const idle = parseIdle(parsed.body);
        if (idle) events.push(idle);
        else out += s.slice(i, parsed.end);
        i = parsed.end;
        continue;
      }
      if (kind === "[") {
        const parsed = consumeCsi(s, i);
        if (parsed.incomplete) {
          this.carry = s.slice(i);
          break;
        }
        const ev = parseCsi(s.slice(i, parsed.end));
        if (ev) events.push(ev);
        out += s.slice(i, parsed.end);
        i = parsed.end;
        continue;
      }
      out += s[i];
      i += 1;
    }
    return { text: out, events };
  }
}

function consumeOsc(s: string, start: number): { end: number; body: string; incomplete: boolean } {
  let i = start + 2;
  while (i < s.length) {
    const c = s.charCodeAt(i);
    if (c === 0x07) {
      return { end: i + 1, body: s.slice(start + 2, i), incomplete: false };
    }
    if (c === 0x1b && i + 1 < s.length && s[i + 1] === "\\") {
      return { end: i + 2, body: s.slice(start + 2, i), incomplete: false };
    }
    i += 1;
  }
  return { end: s.length, body: "", incomplete: true };
}

function consumeCsi(s: string, start: number): { end: number; incomplete: boolean } {
  let i = start + 2;
  while (i < s.length) {
    const code = s.charCodeAt(i);
    if (code >= 0x40 && code <= 0x7e) {
      return { end: i + 1, incomplete: false };
    }
    i += 1;
  }
  return { end: s.length, incomplete: true };
}

function parseIdle(body: string): OscEvent | null {
  if (!body.startsWith("733;twominal;idle;")) return null;
  const cwd = body.slice("733;twominal;idle;".length);
  return { type: "idle", cwd };
}

function parseCsi(seq: string): OscEvent | null {
  // ESC [ ? 1049 h/l  and 47 / 1047
  const m = seq.match(/^\x1b\[\?([0-9;]+)([hl])$/);
  if (!m) return null;
  const modes = m[1].split(";");
  const on = m[2] === "h";
  if (modes.some((x) => x === "1049" || x === "47" || x === "1047")) {
    return { type: "alt", on };
  }
  if (modes.some((x) => x === "1")) {
    return { type: "appCursor", on };
  }
  return null;
}

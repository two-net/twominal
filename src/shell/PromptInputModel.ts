import { normalizeHistoryCommand } from "./history";

const MAX_TRACKED_INPUT_BYTES = 32 * 1024;

export type PromptInputPhase = "inactive" | "prompt" | "editing";

export interface PromptInputSnapshot {
  readonly phase: PromptInputPhase;
  readonly reliable: boolean;
  readonly line: string;
  readonly cursor: number;
}

export interface PromptInputUpdate {
  readonly changed: boolean;
  readonly committedCommand?: string;
}

export class PromptInputModel {
  private phase: PromptInputPhase = "inactive";
  private reliable = false;
  private line = "";
  private cursor = 0;
  private pendingCommand: string | null = null;

  constructor(private readonly nonce: string) {
    if (!nonce) {
      throw new Error("A shell-integration nonce is required.");
    }
  }

  snapshot(): PromptInputSnapshot {
    return {
      phase: this.phase,
      reliable: this.reliable,
      line: this.line,
      cursor: this.cursor,
    };
  }

  handleOsc133(data: string): PromptInputUpdate {
    const parts = data.split(";");
    const marker = parts[0];
    const receivedNonce = marker === "D" ? parts.at(-1) : parts[1];
    if (receivedNonce !== this.nonce) {
      return { changed: false };
    }

    switch (marker) {
      case "A": {
        const committedCommand = this.takePendingCommand();
        this.phase = "prompt";
        this.resetLine(false);
        return updateWithCommand(true, committedCommand);
      }
      case "B":
        this.phase = "editing";
        this.resetLine(true);
        return { changed: true };
      case "C":
      case "D": {
        const committedCommand = this.takePendingCommand();
        this.phase = "inactive";
        this.reliable = false;
        return updateWithCommand(true, committedCommand);
      }
      default:
        return { changed: false };
    }
  }

  handleData(data: string): PromptInputUpdate {
    if (this.phase !== "editing") {
      if (data.length > 0) {
        this.pendingCommand = null;
      }
      return { changed: false };
    }

    if (data === "\r" || data === "\n") {
      this.pendingCommand = this.reliable
        ? normalizeHistoryCommand(this.line)
        : null;
      this.phase = "inactive";
      this.reliable = false;
      return { changed: true };
    }
    if (data === "\u0003") {
      this.pendingCommand = null;
      this.phase = "inactive";
      this.resetLine(false);
      return { changed: true };
    }

    const paste = bracketedPasteContent(data);
    if (paste !== null) {
      if (containsControl(paste)) {
        return this.invalidate();
      }
      return this.insert(paste);
    }

    switch (data) {
      case "\u007f":
      case "\b":
        return this.deleteBackward();
      case "\u0001":
      case "\u001b[H":
      case "\u001b[1~":
        return this.moveCursor(0);
      case "\u0005":
      case "\u001b[F":
      case "\u001b[4~":
        return this.moveCursor(this.line.length);
      case "\u0002":
      case "\u001b[D":
      case "\u001bOD":
        return this.moveCursor(previousGraphemeBoundary(this.line, this.cursor));
      case "\u0006":
      case "\u001b[C":
      case "\u001bOC":
        return this.moveCursor(nextGraphemeBoundary(this.line, this.cursor));
      case "\u0015":
        this.line = this.line.slice(this.cursor);
        this.cursor = 0;
        return { changed: true };
      case "\u000b":
        this.line = this.line.slice(0, this.cursor);
        return { changed: true };
      case "\u0017":
        return this.deletePreviousWord();
      case "\u001b[3~":
        return this.deleteForward();
      default:
        if (containsControl(data)) {
          return this.invalidate();
        }
        return this.insert(data);
    }
  }

  replaceLine(line: string, cursor = line.length): void {
    if (
      this.phase !== "editing" ||
      containsControl(line) ||
      !isGraphemeBoundary(line, cursor)
    ) {
      this.invalidate();
      return;
    }
    this.line = line;
    this.cursor = cursor;
    this.reliable = withinTrackingLimit(line);
  }

  insertSynthetic(value: string): void {
    if (this.phase !== "editing" || containsControl(value)) {
      this.invalidate();
      return;
    }
    this.insert(value);
  }

  private insert(value: string): PromptInputUpdate {
    if (!value) {
      return { changed: false };
    }
    this.line =
      this.line.slice(0, this.cursor) + value + this.line.slice(this.cursor);
    this.cursor += value.length;
    if (!withinTrackingLimit(this.line)) {
      return this.invalidate();
    }
    return { changed: true };
  }

  private deleteBackward(): PromptInputUpdate {
    if (this.cursor === 0) {
      return { changed: false };
    }
    const boundary = previousGraphemeBoundary(this.line, this.cursor);
    this.line = this.line.slice(0, boundary) + this.line.slice(this.cursor);
    this.cursor = boundary;
    return { changed: true };
  }

  private deleteForward(): PromptInputUpdate {
    if (this.cursor >= this.line.length) {
      return { changed: false };
    }
    const boundary = nextGraphemeBoundary(this.line, this.cursor);
    this.line = this.line.slice(0, this.cursor) + this.line.slice(boundary);
    return { changed: true };
  }

  private deletePreviousWord(): PromptInputUpdate {
    let boundary = this.cursor;
    while (boundary > 0 && /\s/u.test(this.line.slice(previousCodePoint(this.line, boundary), boundary))) {
      boundary = previousCodePoint(this.line, boundary);
    }
    while (boundary > 0 && !/\s/u.test(this.line.slice(previousCodePoint(this.line, boundary), boundary))) {
      boundary = previousCodePoint(this.line, boundary);
    }
    this.line = this.line.slice(0, boundary) + this.line.slice(this.cursor);
    this.cursor = boundary;
    return { changed: true };
  }

  private moveCursor(cursor: number): PromptInputUpdate {
    if (cursor === this.cursor) {
      return { changed: false };
    }
    this.cursor = cursor;
    return { changed: true };
  }

  private invalidate(): PromptInputUpdate {
    this.reliable = false;
    return { changed: true };
  }

  private resetLine(reliable: boolean): void {
    this.line = "";
    this.cursor = 0;
    this.reliable = reliable;
  }

  private takePendingCommand(): string | undefined {
    const command = this.pendingCommand ?? undefined;
    this.pendingCommand = null;
    return command;
  }
}

export function countGraphemes(value: string): number {
  return graphemeBoundaries(value).length - 1;
}

export function isGraphemeBoundary(value: string, cursor: number): boolean {
  return (
    Number.isSafeInteger(cursor) &&
    cursor >= 0 &&
    cursor <= value.length &&
    graphemeBoundaries(value).includes(cursor)
  );
}

function updateWithCommand(
  changed: boolean,
  command: string | undefined,
): PromptInputUpdate {
  return command === undefined
    ? { changed }
    : { changed, committedCommand: command };
}

function bracketedPasteContent(data: string): string | null {
  const start = "\u001b[200~";
  const end = "\u001b[201~";
  return data.startsWith(start) && data.endsWith(end)
    ? data.slice(start.length, -end.length)
    : null;
}

function containsControl(value: string): boolean {
  return Array.from(value).some((character) => {
    const point = character.codePointAt(0) ?? 0;
    return point <= 0x1f || (point >= 0x7f && point <= 0x9f);
  });
}

function withinTrackingLimit(value: string): boolean {
  return new TextEncoder().encode(value).byteLength <= MAX_TRACKED_INPUT_BYTES;
}

function graphemeBoundaries(value: string): number[] {
  const boundaries = [0];
  if (typeof Intl.Segmenter === "function") {
    const segmenter = new Intl.Segmenter(undefined, { granularity: "grapheme" });
    for (const segment of segmenter.segment(value)) {
      if (segment.index > 0) {
        boundaries.push(segment.index);
      }
    }
  } else {
    let index = 0;
    for (const character of value) {
      index += character.length;
      boundaries.push(index);
    }
  }
  if (boundaries.at(-1) !== value.length) {
    boundaries.push(value.length);
  }
  return boundaries;
}

function previousGraphemeBoundary(value: string, cursor: number): number {
  const boundaries = graphemeBoundaries(value);
  for (let index = boundaries.length - 1; index >= 0; index -= 1) {
    const boundary = boundaries[index];
    if (boundary !== undefined && boundary < cursor) {
      return boundary;
    }
  }
  return 0;
}

function nextGraphemeBoundary(value: string, cursor: number): number {
  return graphemeBoundaries(value).find((boundary) => boundary > cursor) ?? value.length;
}

function previousCodePoint(value: string, cursor: number): number {
  const previous = cursor - 1;
  if (
    previous > 0 &&
    value.charCodeAt(previous) >= 0xdc00 &&
    value.charCodeAt(previous) <= 0xdfff
  ) {
    return previous - 1;
  }
  return Math.max(0, previous);
}

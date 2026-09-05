const MAX_UNDO_ENTRIES = 100;

export type InputMode = "insert" | "normal" | "visual" | "replace";

export type VimPendingCommand = "d" | "c" | "ci" | null;

export interface VimLineSnapshot {
  readonly line: string;
  readonly cursor: number;
}

export interface VimStateSnapshot extends VimLineSnapshot {
  readonly mode: InputMode;
  readonly pendingCommand: VimPendingCommand;
}

/** The subset of KeyboardEvent used by the input state machine. */
export interface VimKeyEvent {
  readonly key: string;
  readonly ctrlKey?: boolean;
  readonly altKey?: boolean;
  readonly metaKey?: boolean;
  readonly isComposing?: boolean;
}

export type VimHistoryDirection = "previous" | "next";

export type VimInputResult =
  | {
      readonly kind: "passthrough";
      readonly mode: InputMode;
    }
  | {
      readonly kind: "handled";
      readonly mode: InputMode;
      readonly edit: VimLineSnapshot;
    }
  | {
      readonly kind: "history";
      readonly mode: InputMode;
      readonly direction: VimHistoryDirection;
    };

/**
 * Implements Vim-style command-line editing without depending on a shell's own
 * keymap. The terminal controller remains responsible for mirroring returned
 * edits into the PTY.
 */
export class VimInputStateMachine {
  private currentMode: InputMode = "insert";
  private pendingCommand: VimPendingCommand = null;
  private current: VimLineSnapshot = emptyLine();
  private insertCheckpoint: VimLineSnapshot | null = null;
  private readonly undoStack: VimLineSnapshot[] = [];
  private readonly redoStack: VimLineSnapshot[] = [];

  constructor(initial: VimLineSnapshot = emptyLine()) {
    this.beginPrompt(initial);
  }

  get mode(): InputMode {
    return this.currentMode;
  }

  state(): VimStateSnapshot {
    return {
      mode: this.currentMode,
      pendingCommand: this.pendingCommand,
      ...copySnapshot(this.current),
    };
  }

  /** Starts each authenticated shell prompt in Insert mode with fresh undo state. */
  beginPrompt(snapshot: VimLineSnapshot = emptyLine()): void {
    this.currentMode = "insert";
    this.pendingCommand = null;
    this.current = normalizeInsertSnapshot(snapshot);
    this.insertCheckpoint = copySnapshot(this.current);
    this.undoStack.length = 0;
    this.redoStack.length = 0;
  }

  /** Clears transient editing state when the prompt hands control to a program. */
  endPrompt(): void {
    this.pendingCommand = null;
    this.insertCheckpoint = null;
  }

  /**
   * Mirrors input processed outside this state machine (ordinary Insert-mode
   * typing, completion insertion, or history selection).
   */
  observe(snapshot: VimLineSnapshot): VimLineSnapshot {
    this.current =
      this.currentMode === "normal"
        ? normalizeNormalSnapshot(snapshot)
        : normalizeInsertSnapshot(snapshot);
    return copySnapshot(this.current);
  }

  handleKey(event: VimKeyEvent, snapshot: VimLineSnapshot): VimInputResult {
    this.observe(snapshot);

    if (event.isComposing) {
      return this.passthrough();
    }
    if (this.currentMode === "insert") {
      return this.handleInsertKey(event);
    }
    if (this.currentMode === "normal") {
      return this.handleNormalKey(event);
    }

    if (isEscape(event)) {
      this.currentMode = "normal";
      this.pendingCommand = null;
      this.current = normalizeNormalSnapshot(this.current);
      return this.handled();
    }
    return this.passthrough();
  }

  private handleInsertKey(event: VimKeyEvent): VimInputResult {
    if (!isEscape(event)) {
      return this.passthrough();
    }

    this.finishInsertSession();
    this.currentMode = "normal";
    this.pendingCommand = null;
    this.current = normalizeNormalSnapshot({
      line: this.current.line,
      cursor: previousGraphemeBoundary(this.current.line, this.current.cursor),
    });
    return this.handled();
  }

  private handleNormalKey(event: VimKeyEvent): VimInputResult {
    if (isRedo(event)) {
      this.pendingCommand = null;
      return this.restoreFrom(this.redoStack, this.undoStack);
    }

    if (isEscape(event)) {
      this.pendingCommand = null;
      return this.handled();
    }

    if (event.ctrlKey || event.altKey || event.metaKey) {
      this.pendingCommand = null;
      return this.passthrough();
    }

    if (this.pendingCommand !== null) {
      return this.handlePendingKey(event.key);
    }

    switch (event.key) {
      case "Escape":
        return this.handled();
      case "i":
        return this.enterInsert(this.current.cursor);
      case "a":
        return this.enterInsert(
          nextGraphemeBoundary(this.current.line, this.current.cursor),
        );
      case "A":
        return this.enterInsert(this.current.line.length);
      case "I":
        return this.enterInsert(firstNonBlankBoundary(this.current.line));
      case "h":
        return this.moveTo(
          previousGraphemeBoundary(this.current.line, this.current.cursor),
        );
      case "l":
        return this.moveTo(nextNormalBoundary(this.current));
      case "w":
        return this.moveTo(moveWordForward(this.current));
      case "b":
        return this.moveTo(moveWordBackward(this.current));
      case "e":
        return this.moveTo(moveWordEnd(this.current));
      case "0":
        return this.moveTo(0);
      case "$":
        return this.moveTo(lastGraphemeStart(this.current.line));
      case "x":
        return this.deleteCurrentGrapheme();
      case "D":
        return this.deleteToEnd();
      case "d":
        this.pendingCommand = "d";
        return this.handled();
      case "c":
        this.pendingCommand = "c";
        return this.handled();
      case "u":
        return this.restoreFrom(this.undoStack, this.redoStack);
      case "j":
        return this.history("next");
      case "k":
        return this.history("previous");
      case "Enter":
        return this.passthrough();
      default:
        return event.key.length === 1 || consumesInNormalMode(event.key)
          ? this.handled()
          : this.passthrough();
    }
  }

  private handlePendingKey(key: string): VimInputResult {
    const pending = this.pendingCommand;
    this.pendingCommand = null;

    if (pending === "d" && key === "d") {
      return this.applyNormalEdit(emptyLine());
    }
    if (pending === "c" && key === "w") {
      return this.changeRange(changeWordRange(this.current));
    }
    if (pending === "c" && key === "i") {
      this.pendingCommand = "ci";
      return this.handled();
    }
    if (pending === "ci" && key === "w") {
      return this.changeRange(innerWordRange(this.current));
    }
    return this.handled();
  }

  private enterInsert(cursor: number): VimInputResult {
    const checkpoint = copySnapshot(this.current);
    this.currentMode = "insert";
    this.pendingCommand = null;
    this.current = normalizeInsertSnapshot({
      line: this.current.line,
      cursor,
    });
    this.insertCheckpoint = checkpoint;
    return this.handled();
  }

  private moveTo(cursor: number): VimInputResult {
    this.current = normalizeNormalSnapshot({
      line: this.current.line,
      cursor,
    });
    return this.handled();
  }

  private deleteCurrentGrapheme(): VimInputResult {
    if (this.current.line.length === 0) {
      return this.handled();
    }
    const end = nextGraphemeBoundary(this.current.line, this.current.cursor);
    return this.applyNormalEdit(deleteRange(this.current, this.current.cursor, end));
  }

  private deleteToEnd(): VimInputResult {
    if (this.current.cursor >= this.current.line.length) {
      return this.handled();
    }
    return this.applyNormalEdit({
      line: this.current.line.slice(0, this.current.cursor),
      cursor: this.current.cursor,
    });
  }

  private changeRange(range: TextRange): VimInputResult {
    const checkpoint = copySnapshot(this.current);
    const target = deleteRange(this.current, range.start, range.end);
    if (target.line !== checkpoint.line) {
      this.redoStack.length = 0;
    }
    this.currentMode = "insert";
    this.current = normalizeInsertSnapshot(target);
    this.insertCheckpoint = checkpoint;
    return this.handled();
  }

  private applyNormalEdit(target: VimLineSnapshot): VimInputResult {
    const normalized = normalizeNormalSnapshot(target);
    if (normalized.line !== this.current.line) {
      this.pushUndo(this.current);
      this.redoStack.length = 0;
    }
    this.current = normalized;
    return this.handled();
  }

  private finishInsertSession(): void {
    const checkpoint = this.insertCheckpoint;
    this.insertCheckpoint = null;
    if (checkpoint && checkpoint.line !== this.current.line) {
      this.pushUndo(checkpoint);
      this.redoStack.length = 0;
    }
  }

  private restoreFrom(
    source: VimLineSnapshot[],
    destination: VimLineSnapshot[],
  ): VimInputResult {
    const restored = source.pop();
    if (!restored) {
      return this.handled();
    }
    pushBounded(destination, this.current);
    this.current = normalizeNormalSnapshot(restored);
    return this.handled();
  }

  private pushUndo(snapshot: VimLineSnapshot): void {
    pushBounded(this.undoStack, snapshot);
  }

  private handled(): VimInputResult {
    return {
      kind: "handled",
      mode: this.currentMode,
      edit: copySnapshot(this.current),
    };
  }

  private history(direction: VimHistoryDirection): VimInputResult {
    return { kind: "history", mode: this.currentMode, direction };
  }

  private passthrough(): VimInputResult {
    return { kind: "passthrough", mode: this.currentMode };
  }
}

interface Grapheme {
  readonly start: number;
  readonly end: number;
  readonly text: string;
  readonly kind: GraphemeKind;
}

type GraphemeKind = "blank" | "word" | "punctuation";

interface TextRange {
  readonly start: number;
  readonly end: number;
}

function emptyLine(): VimLineSnapshot {
  return { line: "", cursor: 0 };
}

function copySnapshot(snapshot: VimLineSnapshot): VimLineSnapshot {
  return { line: snapshot.line, cursor: snapshot.cursor };
}

function normalizeInsertSnapshot(snapshot: VimLineSnapshot): VimLineSnapshot {
  const cursor = Math.max(0, Math.min(snapshot.cursor, snapshot.line.length));
  return {
    line: snapshot.line,
    cursor: graphemeBoundaryAtOrBefore(snapshot.line, cursor),
  };
}

function normalizeNormalSnapshot(snapshot: VimLineSnapshot): VimLineSnapshot {
  const normalized = normalizeInsertSnapshot(snapshot);
  if (normalized.line.length === 0) {
    return emptyLine();
  }
  return {
    line: normalized.line,
    cursor:
      normalized.cursor === normalized.line.length
        ? lastGraphemeStart(normalized.line)
        : normalized.cursor,
  };
}

function firstNonBlankBoundary(line: string): number {
  return graphemes(line).find((grapheme) => grapheme.kind !== "blank")?.start ?? 0;
}

function lastGraphemeStart(line: string): number {
  return graphemes(line).at(-1)?.start ?? 0;
}

function nextNormalBoundary(snapshot: VimLineSnapshot): number {
  const next = nextGraphemeBoundary(snapshot.line, snapshot.cursor);
  return next === snapshot.line.length
    ? lastGraphemeStart(snapshot.line)
    : next;
}

function moveWordForward(snapshot: VimLineSnapshot): number {
  const parts = graphemes(snapshot.line);
  if (parts.length === 0) {
    return 0;
  }

  let index = graphemeIndexAt(parts, snapshot.cursor);
  const initialKind = parts[index]?.kind;
  if (initialKind === "blank") {
    while (parts[index]?.kind === "blank") {
      index += 1;
    }
  } else {
    while (parts[index + 1]?.kind === initialKind) {
      index += 1;
    }
    index += 1;
    while (parts[index]?.kind === "blank") {
      index += 1;
    }
  }
  return parts[Math.min(index, parts.length - 1)]?.start ?? 0;
}

function moveWordBackward(snapshot: VimLineSnapshot): number {
  const parts = graphemes(snapshot.line);
  if (parts.length === 0 || snapshot.cursor === 0) {
    return 0;
  }

  let index = Math.max(0, graphemeIndexAt(parts, snapshot.cursor) - 1);
  while (index > 0 && parts[index]?.kind === "blank") {
    index -= 1;
  }
  const kind = parts[index]?.kind;
  while (index > 0 && kind !== "blank" && parts[index - 1]?.kind === kind) {
    index -= 1;
  }
  return parts[index]?.start ?? 0;
}

function moveWordEnd(snapshot: VimLineSnapshot): number {
  const parts = graphemes(snapshot.line);
  if (parts.length === 0) {
    return 0;
  }

  let index = graphemeIndexAt(parts, snapshot.cursor);
  const initialKind = parts[index]?.kind;
  if (initialKind !== "blank") {
    const start = index;
    while (parts[index + 1]?.kind === initialKind) {
      index += 1;
    }
    if (index > start) {
      return parts[index]?.start ?? snapshot.cursor;
    }
    index += 1;
  }
  while (parts[index]?.kind === "blank") {
    index += 1;
  }
  if (index >= parts.length) {
    return parts.at(-1)?.start ?? 0;
  }
  const kind = parts[index]?.kind;
  while (parts[index + 1]?.kind === kind) {
    index += 1;
  }
  return parts[index]?.start ?? 0;
}

function changeWordRange(snapshot: VimLineSnapshot): TextRange {
  const parts = graphemes(snapshot.line);
  if (parts.length === 0) {
    return { start: snapshot.cursor, end: snapshot.cursor };
  }

  let index = graphemeIndexAt(parts, snapshot.cursor);
  while (parts[index]?.kind === "blank") {
    index += 1;
  }
  if (index >= parts.length) {
    return { start: snapshot.cursor, end: snapshot.line.length };
  }
  const kind = parts[index]?.kind;
  while (parts[index + 1]?.kind === kind) {
    index += 1;
  }
  return {
    start: snapshot.cursor,
    end: parts[index]?.end ?? snapshot.cursor,
  };
}

function innerWordRange(snapshot: VimLineSnapshot): TextRange {
  const parts = graphemes(snapshot.line);
  if (parts.length === 0) {
    return { start: snapshot.cursor, end: snapshot.cursor };
  }

  let index = graphemeIndexAt(parts, snapshot.cursor);
  if (parts[index]?.kind === "blank") {
    const following = parts.findIndex(
      (part, partIndex) => partIndex >= index && part.kind !== "blank",
    );
    if (following >= 0) {
      index = following;
    } else {
      for (let partIndex = index - 1; partIndex >= 0; partIndex -= 1) {
        if (parts[partIndex]?.kind !== "blank") {
          index = partIndex;
          break;
        }
      }
    }
  }

  const kind = parts[index]?.kind;
  if (!kind || kind === "blank") {
    return { start: snapshot.cursor, end: snapshot.cursor };
  }
  let start = index;
  let end = index;
  while (start > 0 && parts[start - 1]?.kind === kind) {
    start -= 1;
  }
  while (parts[end + 1]?.kind === kind) {
    end += 1;
  }
  return {
    start: parts[start]?.start ?? snapshot.cursor,
    end: parts[end]?.end ?? snapshot.cursor,
  };
}

function deleteRange(
  snapshot: VimLineSnapshot,
  start: number,
  end: number,
): VimLineSnapshot {
  return {
    line: snapshot.line.slice(0, start) + snapshot.line.slice(end),
    cursor: start,
  };
}

function graphemeIndexAt(parts: readonly Grapheme[], cursor: number): number {
  const index = parts.findIndex(
    (part) => cursor >= part.start && cursor < part.end,
  );
  return index >= 0 ? index : Math.max(0, parts.length - 1);
}

function graphemeBoundaryAtOrBefore(line: string, cursor: number): number {
  const parts = graphemes(line);
  for (let index = parts.length - 1; index >= 0; index -= 1) {
    const start = parts[index]?.start;
    if (start !== undefined && start <= cursor) {
      return cursor === line.length ? line.length : start;
    }
  }
  return 0;
}

function previousGraphemeBoundary(line: string, cursor: number): number {
  const parts = graphemes(line);
  for (let index = parts.length - 1; index >= 0; index -= 1) {
    const start = parts[index]?.start;
    if (start !== undefined && start < cursor) {
      return start;
    }
  }
  return 0;
}

function nextGraphemeBoundary(line: string, cursor: number): number {
  return graphemes(line).find((part) => part.start > cursor)?.start ?? line.length;
}

function graphemes(line: string): Grapheme[] {
  const segments: Array<{ start: number; text: string }> = [];
  if (typeof Intl.Segmenter === "function") {
    const segmenter = new Intl.Segmenter(undefined, { granularity: "grapheme" });
    for (const segment of segmenter.segment(line)) {
      segments.push({ start: segment.index, text: segment.segment });
    }
  } else {
    let start = 0;
    for (const text of line) {
      segments.push({ start, text });
      start += text.length;
    }
  }

  return segments.map((segment, index) => ({
    start: segment.start,
    end: segments[index + 1]?.start ?? line.length,
    text: segment.text,
    kind: classifyGrapheme(segment.text),
  }));
}

function classifyGrapheme(value: string): GraphemeKind {
  if (/^\s+$/u.test(value)) {
    return "blank";
  }
  return /^[\p{L}\p{N}\p{M}_]+$/u.test(value) ? "word" : "punctuation";
}

function isEscape(event: VimKeyEvent): boolean {
  return (
    event.key === "Escape" ||
    (event.ctrlKey === true &&
      event.key === "[" &&
      !event.altKey &&
      !event.metaKey)
  );
}

function isRedo(event: VimKeyEvent): boolean {
  return (
    event.ctrlKey === true &&
    !event.altKey &&
    !event.metaKey &&
    event.key.toLowerCase() === "r"
  );
}

function consumesInNormalMode(key: string): boolean {
  return key === "Backspace" || key === "Delete" || key === "Tab";
}

function pushBounded(
  stack: VimLineSnapshot[],
  snapshot: VimLineSnapshot,
): void {
  stack.push(copySnapshot(snapshot));
  if (stack.length > MAX_UNDO_ENTRIES) {
    stack.shift();
  }
}

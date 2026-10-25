// Terminal view: draws one session's frames. Frames are plain data from the Terminal Core and
// are only ever written with textContent and CSSOM, never parsed as HTML.

const FG = -1;
const BG = -2;
const RGB = 0x1000000;

const FLAG = {
  BOLD: 1,
  DIM: 1 << 1,
  ITALIC: 1 << 2,
  UNDERLINE: 1 << 3,
  DOUBLE_UNDERLINE: 1 << 4,
  CURLY_UNDERLINE: 1 << 5,
  DOTTED_UNDERLINE: 1 << 6,
  DASHED_UNDERLINE: 1 << 7,
  STRIKEOUT: 1 << 8,
  INVERSE: 1 << 9,
  HIDDEN: 1 << 10,
  WIDE: 1 << 11,
};
const ANY_UNDERLINE =
  FLAG.UNDERLINE | FLAG.DOUBLE_UNDERLINE | FLAG.CURLY_UNDERLINE | FLAG.DOTTED_UNDERLINE | FLAG.DASHED_UNDERLINE;

export const MODE = { APP_CURSOR: 1, BRACKETED_PASTE: 1 << 1, ALT_SCREEN: 1 << 2, SHOW_CURSOR: 1 << 3 };

const CURSOR_SHAPES = ["block", "underline", "beam", "hollow"];

/** CSS for a colour from a frame: theme variables for defaults and the 16 colours, else RGB. */
export function cssColor(color) {
  if (color === FG) return "var(--term-fg)";
  if (color === BG) return "var(--term-bg)";
  if (color >= RGB) return "#" + (color & 0xffffff).toString(16).padStart(6, "0");
  if (color < 16) return `var(--ansi-${color})`;
  if (color < 232) {
    const index = color - 16;
    const level = (step) => (step === 0 ? 0 : 55 + step * 40);
    return rgb(level(Math.floor(index / 36)), level(Math.floor(index / 6) % 6), level(index % 6));
  }
  const gray = 8 + (color - 232) * 10;
  return rgb(gray, gray, gray);
}

function rgb(r, g, b) {
  return "#" + [r, g, b].map((value) => value.toString(16).padStart(2, "0")).join("");
}

/** Measures one terminal cell in the current font, rounded to device pixels vertically. */
export function measureCell(container) {
  const probe = document.createElement("div");
  probe.className = "terminal";
  probe.style.visibility = "hidden";
  const sample = document.createElement("span");
  sample.style.lineHeight = "var(--term-line-height)";
  sample.style.whiteSpace = "pre";
  sample.textContent = "M".repeat(80);
  probe.append(sample);
  container.append(probe);
  const rect = sample.getBoundingClientRect();
  probe.remove();

  const ratio = window.devicePixelRatio || 1;
  const cell = { width: rect.width / 80, height: Math.round(rect.height * ratio) / ratio };
  document.documentElement.style.setProperty("--cell-w", `${cell.width}px`);
  document.documentElement.style.setProperty("--cell-h", `${cell.height}px`);
  return cell;
}

/** How many cells fit in `container`. */
export function fitGrid(container, cell) {
  const rect = container.getBoundingClientRect();
  return {
    cols: Math.max(1, Math.floor(rect.width / cell.width)),
    rows: Math.max(1, Math.floor(rect.height / cell.height)),
  };
}

export class TerminalView {
  constructor(parent) {
    this.element = document.createElement("div");
    this.element.className = "terminal";
    this.grid = document.createElement("div");
    this.grid.className = "grid";
    this.cursorElement = document.createElement("div");
    this.cursorElement.className = "cursor";
    this.cursorElement.hidden = true;
    this.grid.append(this.cursorElement);
    this.element.append(this.grid);
    parent.append(this.element);

    this.rowElements = [];
    this.rowRuns = [];
    this.mode = 0;
    this.offset = 0;
    this.cursor = null;
  }

  show() {
    this.element.hidden = false;
  }

  hide() {
    this.element.hidden = true;
    this.setShift(0);
  }

  destroy() {
    this.element.remove();
  }

  /** The row the cursor is on, which stands in for the line being edited. */
  cursorRow() {
    return this.cursor ? this.cursor[0] : Math.max(0, this.rowElements.length - 1);
  }

  /** Moves the content up by `rows` to make room for an overlay. Presentation only. */
  setShift(rows) {
    this.grid.style.transform = rows > 0 ? `translateY(calc(${-rows} * var(--cell-h)))` : "";
  }

  applyFrame(frame) {
    this.setRowCount(frame.rows);
    const drawn = new Set();
    for (const [row, runs] of frame.lines) {
      if (row >= frame.rows) continue;
      this.rowRuns[row] = runs;
      this.drawRow(row);
      drawn.add(row);
    }
    // A full frame replaces everything; rows it leaves out are empty.
    if (frame.full) {
      for (let row = 0; row < frame.rows; row++) {
        if (drawn.has(row)) continue;
        this.rowRuns[row] = null;
        this.drawRow(row);
      }
    }
    this.mode = frame.mode;
    this.offset = frame.offset;
    this.placeCursor(frame.cursor);
  }

  setRowCount(count) {
    while (this.rowElements.length < count) {
      const row = document.createElement("div");
      row.className = "row";
      row.style.setProperty("--row", String(this.rowElements.length));
      this.grid.insertBefore(row, this.cursorElement);
      this.rowElements.push(row);
      this.rowRuns.push(null);
    }
    while (this.rowElements.length > count) {
      this.rowElements.pop().remove();
      this.rowRuns.pop();
    }
  }

  drawRow(row) {
    this.rowElements[row].replaceChildren(...(this.rowRuns[row] ?? []).map(drawRun));
  }

  placeCursor(cursor) {
    const previous = this.cursor;
    this.cursor = cursor;
    const element = this.cursorElement;
    if (!cursor) {
      element.hidden = true;
      return;
    }
    const [row, col, shape] = cursor;
    const { text, wide } = this.cellAt(row, col);
    element.hidden = false;
    element.className = `cursor ${CURSOR_SHAPES[shape] ?? "block"}`;
    element.style.left = `calc(${col} * var(--cell-w))`;
    element.style.top = `calc(${row} * var(--cell-h))`;
    element.style.width = wide ? "calc(2 * var(--cell-w))" : "";
    // A block cursor redraws the character it covers in the cursor's text colour.
    element.textContent = shape === 0 ? text : "";
    // Restart the blink whenever the cursor moves, so it is visible while typing.
    if (!previous || previous[0] !== row || previous[1] !== col) void element.offsetWidth;
    element.classList.add("blink");
  }

  cellAt(row, col) {
    for (const [start, text, , , flags] of this.rowRuns[row] ?? []) {
      if (flags & FLAG.WIDE) {
        if (col === start || col === start + 1) return { text, wide: true };
        continue;
      }
      const chars = Array.from(text);
      if (col >= start && col < start + chars.length) return { text: chars[col - start], wide: false };
    }
    return { text: " ", wide: false };
  }

  showFailure(message) {
    const failure = document.createElement("div");
    failure.className = "failure";
    const title = document.createElement("strong");
    title.textContent = "The shell didn’t start";
    const detail = document.createElement("span");
    detail.textContent = message;
    failure.append(title, detail);
    this.grid.replaceChildren(failure);
  }
}

function drawRun([col, text, fg, bg, flags, underlineColor]) {
  const span = document.createElement("span");
  span.textContent = text;
  span.style.left = `calc(${col} * var(--cell-w))`;

  const [fore, back] = flags & FLAG.INVERSE ? [bg, fg] : [fg, bg];
  if (fore !== FG) span.style.color = cssColor(fore);
  if (back !== BG) {
    span.style.backgroundColor = cssColor(back);
    const cells = flags & FLAG.WIDE ? 2 : Array.from(text).length;
    span.style.width = `calc(${cells} * var(--cell-w))`;
  }
  if (flags & FLAG.HIDDEN) span.style.color = "transparent";

  if (flags & FLAG.BOLD) span.classList.add("b");
  if (flags & FLAG.ITALIC) span.classList.add("i");
  if (flags & FLAG.DIM) span.classList.add("d");
  if (flags & FLAG.WIDE) span.classList.add("wide");

  const lines = [];
  if (flags & ANY_UNDERLINE) lines.push("underline");
  if (flags & FLAG.STRIKEOUT) lines.push("line-through");
  if (lines.length) {
    span.style.textDecorationLine = lines.join(" ");
    if (flags & FLAG.DOUBLE_UNDERLINE) span.style.textDecorationStyle = "double";
    else if (flags & FLAG.CURLY_UNDERLINE) span.style.textDecorationStyle = "wavy";
    else if (flags & FLAG.DOTTED_UNDERLINE) span.style.textDecorationStyle = "dotted";
    else if (flags & FLAG.DASHED_UNDERLINE) span.style.textDecorationStyle = "dashed";
    if (underlineColor !== FG) span.style.textDecorationColor = cssColor(underlineColor);
  }
  return span;
}

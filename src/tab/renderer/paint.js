// Renderer (SRS §6.1): painting a frame. src/tab/renderer.js calls it.
//
// Paints what a frame says on a canvas in device pixels. When the rows moved, as output scrolls
// the screen or the user scrolls the view, what was painted is first copied as far as they moved,
// so only the rows the frame sends are painted again. Each row is painted in a band of its own,
// its offset up from its cell and clipped to it, so a row painted alone comes out as it does
// among all the others. The copy and every band are clipped to the grid, so the margins keep the
// background and the rows cut at its edges show what of them is inside (ANIM-06). Each
// cell's text is drawn at the cell's left edge and on the baseline, whole pixels both, so no
// webview's text engine can move a column. A run that may hold ligatures is drawn as one, so the
// font's ligatures form, and stretched across so that each character's glyph still starts at its
// cell's left edge (FONT-04, FONT-05). A glyph from a fallback font that is wider than its cells
// is scaled down to fit them (FONT-11). Those two are the only drawing with a transform. What the
// frame has shapes for is filled as those shapes: box drawing, blocks, powerline symbols,
// underlines and crossing out.
//
// The cursor is painted on a canvas of its own, which src/tab/renderer/cursor.js lays over the
// grid's. The frame's format is written down in crates/tab/src/render/json.rs, which writes it.

// The font of a style: the font's own bold and italic faces where it has them (FONT-09).
function fontOf(full, flags) {
  const italic = flags & 2 ? "italic " : "";
  const bold = flags & 1 ? "bold " : "";
  return `${italic}${bold}${full.fontSize}px ${full.font}`;
}

// How far each glyph advances, by font and then by text. Measured once, and forgotten with
// every frame of every row and whenever a face loads.
let advances = new Map();

// Forgets how far each glyph advances: a face has loaded, so a glyph may now come from it.
export function forget() {
  advances = new Map();
}

// How far `text` advances in `font`, which the context is set to.
function advanceOf(context, font, text) {
  let widths = advances.get(font);
  if (!widths) {
    widths = new Map();
    advances.set(font, widths);
  }
  let width = widths.get(text);
  if (width === undefined) {
    width = context.measureText(text).width;
    widths.set(text, width);
  }
  return width;
}

// Draws `text` in `font`, which the context is set to, over `cells` cells from `x`, on the
// baseline `y`. A glyph from a fallback font may be wider than its cells: then it is scaled
// down, as much across as down, to their width (FONT-11). A glyph is wider when it passes its
// cells by more than half a pixel a cell, a cell's width being the larger of the cell's and
// the face's own "0"'s. So no glyph of the font itself is scaled, not even one of a bold face
// the webview made up.
function fillText(context, font, text, x, y, cells, cellWidth) {
  const width = advanceOf(context, font, text);
  const allowed = cells * (Math.max(cellWidth, advanceOf(context, font, "0")) + 0.5);
  if (width <= allowed) {
    context.fillText(text, x, y);
    return;
  }
  // The clip stays where it was set: a transform moves only what is drawn after it.
  const scale = (cells * cellWidth) / width;
  context.setTransform(scale, 0, 0, scale, x, y);
  context.fillText(text, 0, 0);
  context.setTransform(1, 0, 0, 1, 0, 0);
}

// Draws `text`, a character for each cell from `x`, as one so that the font's ligatures form
// (FONT-04), in `font`, which the context is set to, on the baseline `y`. It is stretched across
// to span exactly its cells, so the glyph of each character starts at its cell's left edge
// (FONT-05). That holds when its glyphs advance alike, as a monospaced font's do, ligatures
// included. When they do not, it draws nothing and returns false: when a character's glyph
// advances other than the face's own "0", as one from a fallback font does, or when the text
// shaped advances other than that many "0"s, as kerning would make it.
function fillJoined(context, font, text, x, y, cellWidth) {
  const advance = advanceOf(context, font, "0");
  for (const char of text) {
    if (Math.abs(advanceOf(context, font, char) - advance) > 1 / 64) {
      return false;
    }
  }
  // Each character is ASCII, so a code unit. The text is measured each time it is drawn: runs
  // are too many to keep.
  const cells = text.length;
  const width = context.measureText(text).width;
  if (!(Math.abs(width - cells * advance) <= cells / 64)) {
    return false;
  }
  const stretch = (cells * cellWidth) / width;
  if (stretch === 1) {
    context.fillText(text, x, y);
    return true;
  }
  // The clip stays where it was set: a transform moves only what is drawn after it.
  context.setTransform(stretch, 0, 0, 1, x, y);
  context.fillText(text, 0, 0);
  context.setTransform(1, 0, 0, 1, 0, 0);
  return true;
}

// Fills `shape`'s pieces with the context's fill, from the cell whose top left is `x`, `y`.
function fillShape(context, shape, x, y) {
  for (const piece of shape) {
    if (piece[0] === "p") {
      context.beginPath();
      context.moveTo(x + piece[1], y + piece[2]);
      for (let i = 3; i < piece.length; i += 2) {
        context.lineTo(x + piece[i], y + piece[i + 1]);
      }
      context.closePath();
      context.fill();
    } else if (piece.length === 5) {
      context.globalAlpha = piece[4];
      context.fillRect(x + piece[0], y + piece[1], piece[2], piece[3]);
      context.globalAlpha = 1;
    } else {
      context.fillRect(x + piece[0], y + piece[1], piece[2], piece[3]);
    }
  }
}

// Keeps the context's font and fill, and sets each only when it changes: a context parses both
// each time they are set. Restoring the context forgets them, and so must its pen.
function penFor(context) {
  return {
    font: null,
    fill: null,
    setFont(value) {
      if (value !== this.font) {
        context.font = this.font = value;
      }
    },
    setFill(value) {
      if (value !== this.fill) {
        context.fillStyle = this.fill = value;
      }
    },
    forget() {
      this.font = null;
      this.fill = null;
    },
  };
}

// Draws each character of `text` on its own, in the cells from `x` of the row whose top is
// `top`: a shape, or a glyph at the cell's left edge. Returns how many cells it took.
function fillEach(context, full, shapes, pen, text, x, top, ink, flags) {
  const [cellWidth] = full.cell;
  let count = 0;
  for (const char of text) {
    const at = x + count * cellWidth;
    const shape = shapes[char];
    if (shape) {
      pen.setFill(ink);
      fillShape(context, shape, at, top);
    } else if (char !== " ") {
      pen.setFont(fontOf(full, flags));
      pen.setFill(ink);
      fillText(context, pen.font, char, at, top + full.baseline, 1, cellWidth);
    }
    count += 1;
  }
  return count;
}

// Draws `run`, one of `frame`'s, in the row whose top is `top`, with column 0 at `left`: its
// text, then its underline and its crossing out.
function paintRun(context, full, frame, pen, run, top, left) {
  const [col, style, text, cells] = run;
  const [cellWidth] = full.cell;
  const { colors, styles, shapes } = frame;
  const [color, flags, underlineColor] = styles[style];
  const ink = colors[color];
  const x = left + col * cellWidth;
  let count = cells;
  if (cells === undefined) {
    // A cell for each character.
    count = fillEach(context, full, shapes, pen, text, x, top, ink, flags);
  } else if (cells === 0) {
    // A cell for each character, drawn as one so that ligatures form, or each on its own
    // when its glyphs do not advance alike.
    pen.setFont(fontOf(full, flags));
    pen.setFill(ink);
    count = text.length;
    if (!fillJoined(context, pen.font, text, x, top + full.baseline, cellWidth)) {
      fillEach(context, full, shapes, pen, text, x, top, ink, flags);
    }
  } else {
    // Drawn whole over its cells.
    pen.setFont(fontOf(full, flags));
    pen.setFill(ink);
    fillText(context, pen.font, text, x, top + full.baseline, cells, cellWidth);
  }

  const underline = flags >> 3;
  if (underline) {
    pen.setFill(colors[underlineColor ?? color]);
    for (let cell = 0; cell < count; cell += 1) {
      fillShape(context, full.underlines[underline - 1], x + cell * cellWidth, top);
    }
  }
  if (flags & 4) {
    pen.setFill(ink);
    for (let cell = 0; cell < count; cell += 1) {
      fillShape(context, full.strikethrough, x + cell * cellWidth, top);
    }
  }
}

// The height of the grid `full` lays out, in device pixels: the rows and the copy are cut there.
function gridHeight(full) {
  return full.grid[1] * full.cell[1];
}

// Paints `frame` with `full`, the layout of the last frame that had every row. A frame with
// every row paints the whole canvas first, margins included. One whose rows moved copies the
// grid as far as they moved first.
export function paint(context, full, frame) {
  const { canvas } = context;
  const [cellWidth, cellHeight] = full.cell;
  const { colors } = frame;
  const pen = penFor(context);
  const grid = gridHeight(full);
  const offset = frame.offset ?? 0;

  context.textAlign = "left";
  context.textBaseline = "alphabetic";
  context.globalAlpha = 1;
  if (frame.full) {
    forget();
    pen.setFill(full.background);
    context.fillRect(0, 0, canvas.width, canvas.height);
  } else if (frame.shift) {
    // Whole device pixels and no transform, so the copy is exact. What it brings in from past
    // the grid's edges is in rows the frame sends.
    context.save();
    context.beginPath();
    context.rect(0, 0, canvas.width, grid);
    context.clip();
    context.drawImage(canvas, 0, -frame.shift);
    context.restore();
  }

  for (const [index, spans, runs] of frame.rows) {
    const top = index * cellHeight - offset;
    const from = Math.max(top, 0);
    const to = Math.min(top + cellHeight, grid);
    if (to <= from) {
      continue;
    }
    context.save();
    context.beginPath();
    context.rect(0, from, canvas.width, to - from);
    context.clip();

    pen.setFill(full.background);
    context.fillRect(0, from, canvas.width, to - from);
    for (let i = 0; i < spans.length; i += 3) {
      pen.setFill(colors[spans[i + 2]]);
      context.fillRect(spans[i] * cellWidth, top, spans[i + 1] * cellWidth, cellHeight);
    }
    for (const run of runs) {
      paintRun(context, full, frame, pen, run, top, 0);
    }

    context.restore();
    pen.forget();
  }
}

// Paints `frame`'s cursor with its cells' top left at `x`, `y`: its shape in its color, and over a
// block the text under it, in the cursor's text color (TERM-07). The text is cut at the cursor's
// cells, so text drawn as one from a cell before them, such as a flag, shows the cursor's part,
// and the cursor is cut at the grid's edges, as its row is. Without `text`, only the shape, as
// while it glides.
export function paintCursor(context, full, frame, x, y, text = true) {
  const { cursor } = frame;
  const [cellWidth, cellHeight] = full.cell;
  const pen = penFor(context);
  const from = Math.max(y, 0);
  const to = Math.min(y + cellHeight, gridHeight(full));

  context.save();
  context.beginPath();
  context.rect(x, from, cursor.cell[2] * cellWidth, Math.max(to - from, 0));
  context.clip();
  context.textAlign = "left";
  context.textBaseline = "alphabetic";
  context.globalAlpha = 1;
  pen.setFill(frame.colors[cursor.color]);
  fillShape(context, cursor.shape, x, y);
  if (text && cursor.text) {
    paintRun(context, full, frame, pen, cursor.text, y, x - cursor.cell[1] * cellWidth);
  }
  context.restore();
}

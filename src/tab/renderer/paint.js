// Renderer (SRS §6.1): painting a frame. src/tab/renderer.js calls it.
//
// Paints what a frame says on a canvas in device pixels. Each row is painted in a band of its
// own, clipped to it, so a row painted alone comes out as it does among all the others. Each
// cell's text is drawn at the cell's left edge and on the baseline, whole pixels both, so no
// webview's text engine can move a column. A run that may hold ligatures is drawn as one, so the
// font's ligatures form, and stretched across so that each character's glyph still starts at its
// cell's left edge (FONT-04, FONT-05). A glyph from a fallback font that is wider than its cells
// is scaled down to fit them (FONT-11). Those two are the only drawing with a transform. What the
// frame has shapes for is filled as those shapes: box drawing, blocks, powerline symbols,
// underlines and crossing out.
//
// The frame's format is written down in crates/tab/src/render/json.rs, which writes it.

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

// Paints `frame` with `full`, the layout of the last frame that had every row. A frame with
// every row paints the whole canvas first, margins included.
export function paint(context, full, frame) {
  const { canvas } = context;
  const [cellWidth, cellHeight] = full.cell;
  const { colors, styles, shapes } = frame;

  // A context parses `font` and `fillStyle` each time they are set, so they are set only when
  // they change. Restoring the context forgets them.
  let font = null;
  let fill = null;
  const setFont = (value) => {
    if (value !== font) {
      context.font = font = value;
    }
  };
  const setFill = (value) => {
    if (value !== fill) {
      context.fillStyle = fill = value;
    }
  };

  // Draws each character of `text` on its own, in the cells from `col` of the row whose top is
  // `top`: a shape, or a glyph at the cell's left edge. Returns how many cells it took.
  const fillEach = (text, col, top, ink, flags) => {
    let count = 0;
    for (const char of text) {
      const x = (col + count) * cellWidth;
      const shape = shapes[char];
      if (shape) {
        setFill(ink);
        fillShape(context, shape, x, top);
      } else if (char !== " ") {
        setFont(fontOf(full, flags));
        setFill(ink);
        fillText(context, font, char, x, top + full.baseline, 1, cellWidth);
      }
      count += 1;
    }
    return count;
  };

  context.textAlign = "left";
  context.textBaseline = "alphabetic";
  context.globalAlpha = 1;
  if (frame.full) {
    forget();
    setFill(full.background);
    context.fillRect(0, 0, canvas.width, canvas.height);
  }

  for (const [index, spans, runs] of frame.rows) {
    const top = index * cellHeight;
    context.save();
    context.beginPath();
    context.rect(0, top, canvas.width, cellHeight);
    context.clip();

    setFill(full.background);
    context.fillRect(0, top, canvas.width, cellHeight);
    for (let i = 0; i < spans.length; i += 3) {
      setFill(colors[spans[i + 2]]);
      context.fillRect(spans[i] * cellWidth, top, spans[i + 1] * cellWidth, cellHeight);
    }

    for (const [col, style, text, cells] of runs) {
      const [color, flags, underlineColor] = styles[style];
      const ink = colors[color];
      let count = cells;
      if (cells === undefined) {
        // A cell for each character.
        count = fillEach(text, col, top, ink, flags);
      } else if (cells === 0) {
        // A cell for each character, drawn as one so that ligatures form, or each on its own
        // when its glyphs do not advance alike.
        setFont(fontOf(full, flags));
        setFill(ink);
        count = text.length;
        if (!fillJoined(context, font, text, col * cellWidth, top + full.baseline, cellWidth)) {
          fillEach(text, col, top, ink, flags);
        }
      } else {
        // Drawn whole over its cells.
        setFont(fontOf(full, flags));
        setFill(ink);
        fillText(context, font, text, col * cellWidth, top + full.baseline, cells, cellWidth);
      }

      const underline = flags >> 3;
      if (underline) {
        setFill(colors[underlineColor ?? color]);
        for (let cell = col; cell < col + count; cell += 1) {
          fillShape(context, full.underlines[underline - 1], cell * cellWidth, top);
        }
      }
      if (flags & 4) {
        setFill(ink);
        for (let cell = col; cell < col + count; cell += 1) {
          fillShape(context, full.strikethrough, cell * cellWidth, top);
        }
      }
    }

    context.restore();
    font = null;
    fill = null;
  }
}

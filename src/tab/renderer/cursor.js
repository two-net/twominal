// Renderer (SRS §6.1): the cursor. src/tab/renderer.js calls it.
//
// Draws the frame's cursor on a canvas of its own, laid over the grid's, so that it blinks,
// fades and glides without a row being painted again. The two canvases are the same size, in
// the same place, so the cursor falls on the grid's device pixels exactly at any scale (HW-03).
// The Rust half says the cursor's cells, shape and colors, the text under it, and whether it
// blinks, fades and glides (TERM-07, ANIM-07, ANIM-08); here that is only timed. A blink shows
// and hides the canvas, a fade changes how opaque it is, and a glide draws the cursor again at
// each display frame, on whole device pixels. While the cursor is steady or hidden no
// timer and no animation frame runs, and while it blinks without fading only a timer does
// (PERF-08). It is drawn the frame's offset up, as the rows are, and the grid cuts it; the Rust
// half says it does not glide when the view moved. DESIGN.md §3 has the rules.

import { paintCursor } from "./paint.js";

// How long the cursor is shown, and then hidden, as it blinks.
const BLINK = 600;
// How long a fading blink takes to fade out or in: no longer than a cross-fade under reduced
// motion may take (ANIM-02).
const FADE = 150;
// How long the cursor takes to glide to its cell.
const GLIDE = 100;

export class Cursor {
  #canvas;
  #context;
  // What the canvas draws: the last frame's cursor, with the colors, styles and shapes it names,
  // and the layout it was laid out with. Null while no cursor is drawn.
  #drawn = null;
  #full = null;
  // Where the cursor is drawn, in device pixels from the top left of the grid, and the box it
  // covers there.
  #at = null;
  #box = null;
  // Whether the blink shows the cursor.
  #shown = true;
  #timer = 0;
  #request = 0;
  // The fade under way, from one opacity to another, and the glide, from one place to another,
  // each with when it started.
  #fade = null;
  #glide = null;

  // A cursor in the terminal area `container`, over the grid's canvas.
  constructor(container) {
    this.#canvas = document.createElement("canvas");
    this.#canvas.width = 0;
    this.#canvas.height = 0;
    Object.assign(this.#canvas.style, {
      display: "none",
      position: "absolute",
      top: "0",
      left: "0",
      pointerEvents: "none",
    });
    container.append(this.#canvas);
    this.#context = this.#canvas.getContext("2d");
  }

  // Draws the cursor of `frame`, which was painted with `full`, the layout of the last frame that
  // had every row, or takes it away when the frame has none.
  show(frame, full) {
    const cursor = frame.cursor;
    if (!cursor) {
      this.#hide();
      return;
    }
    const before = this.#drawn?.cursor;
    const layout = this.#full;
    const { colors, styles, shapes } = frame;
    this.#drawn = { colors, styles, shapes, cursor };
    this.#full = full;

    // As large as the grid's canvas, which changing clears.
    const canvas = this.#canvas;
    const [width, height] = full.canvas;
    if (canvas.width !== width || canvas.height !== height) {
      canvas.width = width;
      canvas.height = height;
      this.#box = null;
    }
    canvas.style.width = `${width / full.scale}px`;
    canvas.style.height = `${height / full.scale}px`;
    canvas.style.display = "block";

    // A glide starts where the cursor is, so one cut short goes on from there (ANIM-04). It
    // needs a cell the cursor was drawn in, on a grid of the same cells.
    const [row, col] = cursor.cell;
    const [cellWidth, cellHeight] = full.cell;
    const to = [col * cellWidth, row * cellHeight - (frame.offset ?? 0)];
    const same =
      layout &&
      layout.scale === full.scale &&
      layout.cell[0] === cellWidth &&
      layout.cell[1] === cellHeight;
    const moved = !this.#at || this.#at[0] !== to[0] || this.#at[1] !== to[1];
    if (cursor.glide && before && same && moved) {
      this.#glide = { from: this.#at, to, start: performance.now() };
      this.#draw(this.#at, false);
      this.#ask();
    } else {
      this.#glide = null;
      this.#draw(to, true);
    }

    // A cursor that moved or changed is shown at once, and blinks again from there.
    const changed =
      !before ||
      before.color !== cursor.color ||
      before.cell.join() !== cursor.cell.join() ||
      JSON.stringify(before.shape) !== JSON.stringify(cursor.shape);
    if (!cursor.blink) {
      this.#still();
    } else if (changed || !this.#timer || !before.fade !== !cursor.fade) {
      this.#still();
      this.#timer = setTimeout(this.#blink, BLINK);
    }
  }

  destroy() {
    this.#hide();
    this.#canvas.remove();
  }

  // Takes the cursor away, and stops everything that moves it.
  #hide() {
    this.#still();
    this.#glide = null;
    cancelAnimationFrame(this.#request);
    this.#request = 0;
    this.#drawn = null;
    this.#at = null;
    this.#canvas.style.display = "none";
  }

  // Shows the cursor whole, with no blink or fade under way.
  #still() {
    clearTimeout(this.#timer);
    this.#timer = 0;
    this.#shown = true;
    this.#fade = null;
    this.#canvas.style.visibility = "visible";
    this.#canvas.style.opacity = "1";
  }

  // Draws the cursor with its top left at `at`, in device pixels, where it was drawn last
  // taken away: with the text under it, or, while it glides, without.
  #draw(at, text) {
    const context = this.#context;
    if (this.#box) {
      context.clearRect(...this.#box);
    }
    const [cellWidth, cellHeight] = this.#full.cell;
    this.#box = [at[0], at[1], this.#drawn.cursor.cell[2] * cellWidth, cellHeight];
    this.#at = at;
    paintCursor(context, this.#full, this.#drawn, at[0], at[1], text);
  }

  // The blink's next half: the cursor hidden or shown, at once or fading.
  #blink = () => {
    this.#timer = setTimeout(this.#blink, BLINK);
    this.#shown = !this.#shown;
    if (this.#drawn.cursor.fade) {
      const from = Number(this.#canvas.style.opacity);
      this.#fade = { from, to: this.#shown ? 1 : 0, start: performance.now() };
      this.#ask();
    } else {
      this.#canvas.style.visibility = this.#shown ? "visible" : "hidden";
    }
  };

  // Asks for the next display frame, for the glide or the fade under way.
  #ask() {
    if (!this.#request) {
      this.#request = requestAnimationFrame(this.#step);
    }
  }

  // One display frame of the glide and the fade under way.
  #step = (now) => {
    this.#request = 0;
    const done = (duration, start) => Math.min(1, Math.max(0, (now - start) / duration));
    if (this.#glide) {
      const { from, to, start } = this.#glide;
      const t = done(GLIDE, start);
      // Eased out: fast from where it was, slowing into its cell, where it draws its text.
      const eased = 1 - (1 - t) ** 3;
      const at = from.map((value, axis) => Math.round(value + (to[axis] - value) * eased));
      if (t === 1) {
        this.#glide = null;
      }
      this.#draw(at, !this.#glide);
    }
    if (this.#fade) {
      const { from, to, start } = this.#fade;
      const t = done(FADE, start);
      this.#canvas.style.opacity = String(from + (to - from) * t);
      if (t === 1) {
        this.#fade = null;
      }
    }
    if (this.#glide || this.#fade) {
      this.#ask();
    }
  };
}

// Renderer (SRS §6.1), the half that paints. Its Rust half is crates/tab/src/render.rs.
//
// Draws the Terminal Core's state and the command line with the active font, ligatures and
// palette. Must not change terminal state.
//
// This half decides nothing a test in Rust could check. It measures the font and the terminal
// area in device pixels and reports them; the Rust half lays the grid out and sends frames; this
// half paints them on a canvas with a pixel for each of the display's, so text is sharp at any
// scale. When the window moves to a display of another scale it measures again, and the frame
// laid out for that scale is painted crisp (HW-03). DESIGN.md §3 has the rules.
//
// A frame is painted the moment it arrives, and no output fades or moves (ANIM-05). Only the
// cursor does: it blinks, and fades and glides when its settings say, on a canvas of its own
// that renderer/cursor.js lays over the grid (ANIM-07, ANIM-08). That a frame was drawn is
// reported at the display's next frame, so frames never come faster than the display shows
// them, and a hidden window, whose frames are paused, asks for none (PERF-07).
//
// The App Shell's page creates it in the terminal area, gives it the font the App Shell sends,
// and passes what it measures and that a frame was drawn to the Rust half (#31). The font is a
// CSS family list ending in the bundled JetBrains Mono and the OS's monospace, so a character
// the chosen font lacks comes from a fallback font, and paint.js fits it to its cells (FONT-11).
// Filled in by #22 (grid), #23 (fonts), #24 (ligatures) and #25 (cursor).

import { Cursor } from "./renderer/cursor.js";
import { forget, paint } from "./renderer/paint.js";

// The faces of a font: regular, bold, italic and both, as CSS names them.
const FACES = ["", "bold ", "italic ", "italic bold "];

// A canvas of its own to measure fonts on, so measuring leaves the painted one as it is.
let measuring = null;

export class Renderer {
  #container;
  #canvas;
  #context;
  #cursor;
  #onMeasure;
  #onDrawn;
  // The font, as CSS names it, and its size in CSS pixels: the bundled font until the App
  // Shell's page gives it the Font settings.
  #font = { family: '"JetBrains Mono", monospace', size: 13 };
  // The terminal area, as the ResizeObserver saw it last: in CSS pixels, and in device pixels
  // where the webview says, with the scale it was seen at.
  #area = null;
  // The layout of the last frame with every row, which every frame is painted with.
  #full = null;
  #caret = null;
  #observer;
  #unwatch = () => {};
  // Each measuring is counted, so that only the last reports when fonts load slowly.
  #measured = 0;
  #destroyed = false;

  // A Renderer for the terminal area `container`. `onMeasure` is given what is measured, for
  // the Rust half to lay out, and `onDrawn` is called when a frame has been drawn.
  constructor(container, { onMeasure, onDrawn }) {
    this.#container = container;
    this.#onMeasure = onMeasure;
    this.#onDrawn = onDrawn;

    // No pixels until the first frame says how many: a new canvas would show black.
    this.#canvas = document.createElement("canvas");
    this.#canvas.width = 0;
    this.#canvas.height = 0;
    Object.assign(this.#canvas.style, {
      display: "block",
      position: "absolute",
      top: "0",
      left: "0",
    });
    container.append(this.#canvas);
    this.#context = this.#canvas.getContext("2d", { alpha: false });
    this.#cursor = new Cursor(container);

    this.#observer = new ResizeObserver(([entry]) => this.#resized(entry));
    try {
      // WebView2 says exactly how many device pixels the area covers. WebKit does not know
      // the box, and throws.
      this.#observer.observe(container, { box: "device-pixel-content-box" });
    } catch {
      this.#observer.observe(container);
    }
    this.#watchScale();
    document.fonts.addEventListener("loadingdone", this.#fontsLoaded);
  }

  // The font to draw with: a family, or a list of them, as CSS names it, and its size in CSS
  // pixels. The App Shell makes both from the Font and Font size settings (FONT-02).
  setFont(family, size) {
    this.#font = { family, size };
    this.#measure();
  }

  // Paints `frame`, the Rust half's JSON read, at once.
  draw(frame) {
    try {
      if (frame.full) {
        this.#lay(frame.full);
      }
      if (this.#full) {
        paint(this.#context, this.#full, frame);
        this.#cursor.show(frame, this.#full);
      }
      this.#caret = frame.caret;
    } finally {
      // Even a frame that could not be painted is reported, so no frame can stop the tab.
      requestAnimationFrame(() => {
        if (!this.#destroyed) {
          this.#onDrawn();
        }
      });
    }
  }

  // The caret's cell, in CSS pixels from the top left of the terminal area: where the text
  // being composed starts, or the cursor. The Input Handler's textarea follows it (#31). Null
  // before the first frame.
  get caret() {
    const full = this.#full;
    if (!full || !this.#caret) {
      return null;
    }
    const [row, col] = this.#caret;
    const [width, height] = full.cell;
    return {
      x: (col * width) / full.scale,
      y: (row * height) / full.scale,
      width: width / full.scale,
      height: height / full.scale,
    };
  }

  destroy() {
    this.#destroyed = true;
    this.#observer.disconnect();
    this.#unwatch();
    document.fonts.removeEventListener("loadingdone", this.#fontsLoaded);
    this.#cursor.destroy();
    this.#canvas.remove();
  }

  // Makes the canvas the size a frame with every row was laid out for. Changing its size
  // clears it and its context, so only such a frame does it, in the task that paints it all.
  #lay(full) {
    const canvas = this.#canvas;
    const [width, height] = full.canvas;
    if (canvas.width !== width || canvas.height !== height) {
      canvas.width = width;
      canvas.height = height;
    }
    // A pixel of the canvas for each of the display's, so nothing is resampled.
    canvas.style.width = `${width / full.scale}px`;
    canvas.style.height = `${height / full.scale}px`;
    // What the canvas does not cover, as while the window is being made larger.
    this.#container.style.backgroundColor = full.background;
    this.#full = full;
  }

  #resized(entry) {
    const device = entry.devicePixelContentBoxSize?.[0];
    this.#area = {
      width: entry.contentRect.width,
      height: entry.contentRect.height,
      device: device ? [device.inlineSize, device.blockSize] : null,
      scale: window.devicePixelRatio,
    };
    this.#measure();
  }

  // Measures again when the window moves to a display of another scale. WebKit's
  // ResizeObserver says nothing when only the scale changed.
  #watchScale() {
    const query = matchMedia(`(resolution: ${window.devicePixelRatio}dppx)`);
    const changed = () => {
      this.#watchScale();
      this.#measure();
    };
    query.addEventListener("change", changed, { once: true });
    this.#unwatch = () => query.removeEventListener("change", changed);
  }

  // A face that loaded may draw glyphs a fallback font drew before, at another width.
  #fontsLoaded = () => {
    forget();
    this.#measure();
  };

  // Measures the font at its size in device pixels, and the terminal area, once the font's
  // faces have loaded, and reports them.
  async #measure() {
    const turn = ++this.#measured;
    const area = this.#area;
    if (!area) {
      return;
    }
    const scale = window.devicePixelRatio || 1;
    const { family, size } = this.#font;
    const fontSize = size * scale;
    const font = `${fontSize}px ${family}`;
    try {
      await Promise.all(FACES.map((face) => document.fonts.load(face + font)));
    } catch {
      // A face that cannot load is drawn as the webview falls back, and measured so.
    }
    if (turn !== this.#measured || this.#destroyed) {
      return;
    }

    measuring ??= document.createElement("canvas").getContext("2d");
    measuring.font = font;
    const metrics = measuring.measureText("0");
    // The size in device pixels is the size in CSS pixels times the scale, as the display
    // rounds it. Where the webview says otherwise, as headless Chromium does when the scale is
    // only pretended, or said it at another scale, the product is taken.
    const product = [Math.round(area.width * scale), Math.round(area.height * scale)];
    const exact =
      area.device &&
      area.scale === scale &&
      area.device.every((size, axis) => Math.abs(size - product[axis]) <= 1);
    const [width, height] = exact ? area.device : product;
    this.#onMeasure({
      scale,
      font: family,
      fontSize,
      advance: metrics.width,
      ascent: metrics.fontBoundingBoxAscent ?? fontSize * 0.8,
      descent: metrics.fontBoundingBoxDescent ?? fontSize * 0.2,
      width,
      height,
    });
  }
}

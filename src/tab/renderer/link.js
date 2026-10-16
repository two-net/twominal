// Renderer (SRS §6.1): where the link under the mouse leads. src/tab/renderer.js calls it.
//
// Shows the frame's link target in a bubble at the bottom left of the terminal area, or at its
// top left while the mouse is in the lower half of the view, away from the link, and below the
// find bar while it is open, in the overlay above the grid, which takes no room from it (UI-02). The Rust half says what it holds:
// the URL the OS is handed if the link is clicked, or the link's own target when it may not
// open, and whether it opens (TERM-20, SEC-07). Here that is only laid out. The whole target is
// shown, wrapped over as many lines as it takes, so the user sees where a link leads before
// opening it. It is text, never a link of the page's, and takes no click. It appears and goes at
// once (ANIM-05), and its colors are the system's until the Theme Manager brings the palettes
// (#39). DESIGN.md §3 has the rules.

// What the bubble says under a link that does not open.
const NOT_OPENED = "Twominal opens only web, mail and local file links.";

export class LinkTarget {
  #bubble;
  #target;
  #note;

  // A bubble in `overlay`, the layer over the grid, hidden until a frame has a link.
  constructor(overlay) {
    const bubble = element("div", {
      display: "none",
      position: "absolute",
      left: "8px",
      maxWidth: "calc(100% - 16px)",
      boxSizing: "border-box",
      padding: "3px 8px",
      border: "1px solid GrayText",
      borderRadius: "4px",
      background: "Canvas",
      color: "CanvasText",
      font: "12px/1.4 system-ui, sans-serif",
      pointerEvents: "none",
      userSelect: "none",
    });
    bubble.setAttribute("role", "tooltip");
    // Monospaced, so that characters that look alike in other fonts are told apart.
    this.#target = element("div", {
      font: "12px/1.4 ui-monospace, monospace",
      whiteSpace: "pre-wrap",
      overflowWrap: "anywhere",
    });
    this.#note = element("div", { display: "none", color: "GrayText" });
    this.#note.textContent = NOT_OPENED;
    bubble.append(this.#target, this.#note);
    overlay.append(bubble);
    this.#bubble = bubble;
  }

  // Shows the link `link` of a frame, or hides the bubble when the frame has none: the mouse is
  // over no link. At the top, it goes `below` that many CSS pixels: the find bar's bottom.
  show(link, below = 0) {
    const style = this.#bubble.style;
    if (!link) {
      style.display = "none";
      return;
    }
    style.display = "block";
    style.top = link.top ? `${below + 8}px` : "";
    style.bottom = link.top ? "" : "8px";
    // Only when it changed, as the text is laid out again.
    if (this.#target.textContent !== link.target) {
      this.#target.textContent = link.target;
    }
    this.#note.style.display = link.opens ? "none" : "block";
  }

  destroy() {
    this.#bubble.remove();
  }
}

// An element of `tag` with the inline `style` given.
function element(tag, style) {
  const made = document.createElement(tag);
  Object.assign(made.style, style);
  return made;
}

// Renderer (SRS §6.1): the find bar. src/tab/renderer.js calls it.
//
// Draws the frame's find bar at the top right of the terminal area, in the overlay above the
// grid, so it takes no room from the grid (UI-02). The Rust half says what it holds: the query
// cut at the caret, with the text an input method is composing there between, which match is
// current and how many there are, the toggles, and whether the bar has the keys (TERM-18,
// TERM-19). Here that is only laid out. The keys stay with the Input Handler's textarea, and the
// Input Handler edits the query in Rust, so the field is no text field of the page's. A click on
// a button or in the field is the Input Handler's webview half's to send, as its `data-find`
// names it (#31). The bar appears and goes at once until the Animator slides it (#40, ANIM-01),
// and its colors are the system's until the Theme Manager brings the palettes (#39).
// DESIGN.md §3 has the rules.

// The buttons, in their order: what each sends, its name for screen readers (ACC-03), and what
// it shows. The two toggles say whether they are on.
const BUTTONS = [
  ["case", "Match case", "Aa", true],
  ["regex", "Regular expression", ".*", true],
  ["up", "Previous match", "↑", false],
  ["down", "Next match", "↓", false],
  ["close", "Close", "×", false],
];

export class FindBar {
  #bar;
  #field;
  #before;
  #composing;
  #caret;
  #after;
  #count;
  #toggles = {};
  #focused = false;

  // A find bar in `overlay`, the layer over the grid, hidden until a frame has one.
  constructor(overlay) {
    const bar = element("div", {
      display: "none",
      position: "absolute",
      top: "8px",
      right: "16px",
      maxWidth: "calc(100% - 32px)",
      alignItems: "center",
      gap: "4px",
      padding: "4px",
      border: "1px solid GrayText",
      borderRadius: "6px",
      background: "Canvas",
      color: "CanvasText",
      font: "13px/1.4 system-ui, sans-serif",
      pointerEvents: "auto",
      userSelect: "none",
    });
    bar.setAttribute("role", "search");
    bar.setAttribute("aria-label", "Find");

    // The field: the query and its caret, which the Rust half places.
    this.#field = element("div", {
      position: "relative",
      flex: "1 1 auto",
      minWidth: "160px",
      overflow: "hidden",
      whiteSpace: "pre",
      padding: "2px 6px",
      border: "1px solid GrayText",
      borderRadius: "4px",
      cursor: "text",
    });
    this.#field.dataset.find = "focus";
    this.#field.setAttribute("role", "textbox");
    this.#field.setAttribute("aria-label", "Find");
    this.#before = element("span", {});
    this.#composing = element("span", { textDecoration: "underline" });
    this.#caret = element("span", {
      display: "inline-block",
      width: "1px",
      height: "1.2em",
      marginRight: "-1px",
      verticalAlign: "text-bottom",
      background: "CanvasText",
    });
    this.#after = element("span", {});
    this.#field.append(this.#before, this.#composing, this.#caret, this.#after);

    // How many matches there are, read out when it changes.
    this.#count = element("span", {
      flex: "none",
      minWidth: "6em",
      padding: "0 4px",
      textAlign: "right",
    });
    this.#count.setAttribute("aria-live", "polite");
    bar.append(this.#field, this.#count);

    for (const [name, label, shows, toggle] of BUTTONS) {
      const button = element("button", {
        flex: "none",
        minWidth: "24px",
        padding: "2px 4px",
        border: "1px solid transparent",
        borderRadius: "4px",
        background: "transparent",
        color: "inherit",
        font: "inherit",
      });
      button.type = "button";
      // The keys stay with the textarea, so a button never takes the focus.
      button.tabIndex = -1;
      button.dataset.find = name;
      button.setAttribute("aria-label", label);
      button.textContent = shows;
      if (toggle) {
        this.#toggles[name] = button;
      }
      bar.append(button);
    }
    overlay.append(bar);
    this.#bar = bar;
  }

  // Shows the find bar `find` of a frame, or hides it when the frame has none: the bar is closed.
  show(find) {
    if (!find) {
      this.#bar.style.display = "none";
      this.#focused = false;
      return;
    }
    this.#bar.style.display = "flex";
    const [before, composing, after] = find.query;
    this.#before.textContent = before;
    this.#composing.textContent = composing;
    this.#after.textContent = after;
    // The whole query selected, as a text field shows it.
    const selected = find.selected ? { background: "Highlight", color: "HighlightText" } : {};
    for (const part of [this.#before, this.#after]) {
      part.style.background = selected.background ?? "";
      part.style.color = selected.color ?? "";
    }
    // The caret and the focus ring only while the bar has the keys (ACC-03).
    this.#focused = Boolean(find.focused);
    this.#caret.style.visibility = this.#focused && !find.selected ? "visible" : "hidden";
    this.#field.style.outline = this.#focused ? "2px solid Highlight" : "none";
    this.#field.style.borderColor = find.invalid ? "red" : "GrayText";
    this.#field.setAttribute("aria-invalid", String(Boolean(find.invalid)));
    // Only when it changed, so that a screen reader does not read it out at every frame.
    const count = counted(find, before + composing + after);
    if (this.#count.textContent !== count) {
      this.#count.textContent = count;
    }
    this.#reveal();
    for (const [name, button] of Object.entries(this.#toggles)) {
      const on = Boolean(find[name]);
      button.setAttribute("aria-pressed", String(on));
      button.style.background = on ? "Highlight" : "transparent";
      button.style.color = on ? "HighlightText" : "inherit";
    }
  }

  // Scrolls a long query in its field so that the caret, and what is composed before it, shows.
  #reveal() {
    const field = this.#field;
    const at = this.#caret.offsetLeft;
    const room = field.clientWidth - 8;
    if (at - field.scrollLeft > room) {
      field.scrollLeft = at - room;
    } else if (at - field.scrollLeft < 8) {
      field.scrollLeft = Math.max(0, at - 8);
    }
  }

  // How far down the terminal area the bar reaches, in CSS pixels, or 0 while it is hidden, so
  // that what else lies at the top goes below it.
  bottom() {
    const bar = this.#bar;
    return bar.style.display === "none" ? 0 : bar.offsetTop + bar.offsetHeight;
  }

  // The caret's box in CSS pixels from the top left of `container`, the terminal area, while
  // the bar has the keys: where the textarea goes, so that an input method opens its window
  // there (#31). Null otherwise.
  caret(container) {
    if (!this.#focused) {
      return null;
    }
    const caret = this.#caret.getBoundingClientRect();
    const area = container.getBoundingClientRect();
    return {
      x: caret.left - area.left,
      y: caret.top - area.top,
      width: caret.width,
      height: caret.height,
    };
  }

  destroy() {
    this.#bar.remove();
  }
}

// What the bar says of the matches: none, no pattern, or the current one of how many, with a
// plus when there are more than are kept.
function counted(find, query) {
  const [current, count] = find.match;
  if (find.invalid) {
    return "Not a pattern";
  }
  if (query === "") {
    return "";
  }
  if (count === 0) {
    return "No matches";
  }
  return `${current} of ${count.toLocaleString()}${find.more ? "+" : ""}`;
}

// An element of `tag` with the inline `style` given.
function element(tag, style) {
  const made = document.createElement(tag);
  Object.assign(made.style, style);
  return made;
}

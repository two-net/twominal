// Mode indicators (UI-05): Vim mode, private mode, and the smart command line being off or
// unavailable. Each feature reports its state per tab; nothing shows until one does.

const VIM_MODES = { normal: "Normal", insert: "Insert", visual: "Visual", replace: "Replace" };
const SCL_STATES = { off: "Smart command line off", unavailable: "Plain terminal" };

/**
 * The indicators to show for a tab's state, in display order.
 *
 * @param {{vimMode?: string|null, private?: boolean, scl?: string|null}} state
 */
export function indicatorItems(state = {}) {
  const items = [];
  if (state.vimMode && VIM_MODES[state.vimMode]) items.push({ key: "vim", label: VIM_MODES[state.vimMode] });
  if (state.private) items.push({ key: "private", label: "Private" });
  if (state.scl && SCL_STATES[state.scl]) items.push({ key: "scl", label: SCL_STATES[state.scl] });
  return items;
}

/** Renders the active tab's indicators as a corner overlay. */
export class Indicators {
  /** @param {import("./overlays.js").OverlayHost} host */
  constructor(host) {
    this.host = host;
    this.element = document.createElement("div");
    this.element.className = "indicators";
    this.element.setAttribute("role", "status");
  }

  render(state) {
    const items = indicatorItems(state);
    if (items.length === 0) {
      this.host.hide("indicators");
      return;
    }
    this.element.replaceChildren(
      ...items.map(({ key, label }) => {
        const pill = document.createElement("span");
        pill.className = `indicator indicator-${key}`;
        pill.textContent = label;
        return pill;
      }),
    );
    if (!this.host.has("indicators")) this.host.show("indicators", this.element, { anchor: "corner" });
  }
}

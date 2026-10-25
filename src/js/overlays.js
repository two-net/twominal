// Overlay placement (UI-04): overlays never hide the line being edited.
//
// Until the smart command line exists, the line being edited is the cursor row.

/**
 * Places an overlay `height` rows tall on a view `rows` rows tall.
 *
 * - `below`: just under the edit row. When there is not enough room, `shift` says how many rows
 *   to move the content up; `top` is then relative to the shifted view.
 * - `top` / `bottom`: a bar at that edge, moved to the other edge if it would cover the edit row.
 * - `corner`: a one-row badge at the bottom right, or the top right when the edit row is last.
 *
 * Returns `{ top, height, shift, edge }`, or `{ hidden: true }` when the view is too small.
 */
export function placeOverlay({ rows, editRow, height, anchor }) {
  if (rows < 2) return { hidden: true };
  const row = clamp(editRow, 0, rows - 1);
  const fit = clamp(height, 1, rows - 1);

  switch (anchor) {
    case "below": {
      const room = rows - 1 - row;
      const shift = Math.max(0, fit - room);
      return { top: row - shift + 1, height: fit, shift, edge: "below" };
    }
    case "top":
    case "bottom": {
      const atTop = { top: 0, height: fit, shift: 0, edge: "top" };
      const atBottom = { top: rows - fit, height: fit, shift: 0, edge: "bottom" };
      const [first, second] = anchor === "top" ? [atTop, atBottom] : [atBottom, atTop];
      if (!covers(first, row)) return first;
      if (!covers(second, row)) return second;
      return placeOverlay({ rows, editRow: row, height: fit, anchor: "below" });
    }
    default: {
      const top = row === rows - 1 ? 0 : rows - 1;
      return { top, height: 1, shift: 0, edge: top === 0 ? "top" : "bottom" };
    }
  }
}

/** Whether a placement covers `row`. */
export function covers(placement, row) {
  return !placement.hidden && row >= placement.top && row < placement.top + placement.height;
}

function clamp(value, min, max) {
  return Math.max(min, Math.min(max, value));
}

/**
 * Mounts overlays over the terminal and keeps them placed as the cursor moves.
 * Shifting is presentation only: the terminal view is moved, never the grid itself.
 */
export class OverlayHost {
  /**
   * @param {HTMLElement} layer
   * @param {{ metrics: () => {rows: number, editRow: number}, setShift: (rows: number) => void }} view
   */
  constructor(layer, view) {
    this.layer = layer;
    this.view = view;
    this.overlays = new Map();
  }

  /** Shows `element` as overlay `id`. `height` is in rows. */
  show(id, element, { anchor, height = 1 }) {
    this.hide(id);
    this.overlays.set(id, { element, anchor, height });
    this.layer.append(element);
    this.layout();
  }

  hide(id) {
    const overlay = this.overlays.get(id);
    if (!overlay) return;
    overlay.element.remove();
    this.overlays.delete(id);
    this.layout();
  }

  has(id) {
    return this.overlays.has(id);
  }

  layout() {
    const { rows, editRow } = this.view.metrics();
    const placements = [...this.overlays.values()].map((overlay) => [
      overlay,
      placeOverlay({ rows, editRow, height: overlay.height, anchor: overlay.anchor }),
    ]);
    const shift = Math.max(0, ...placements.map(([, placement]) => placement.shift ?? 0));
    for (const [overlay, placement] of placements) {
      overlay.element.hidden = Boolean(placement.hidden);
      if (placement.hidden) continue;
      // Every `below` overlay shares the largest shift, so place it from that.
      const top = placement.edge === "below" ? editRow - shift + 1 : placement.top;
      overlay.element.style.top = `calc(${top} * var(--cell-h))`;
      overlay.element.dataset.edge = placement.edge;
    }
    this.view.setShift(shift);
  }
}

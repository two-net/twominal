import assert from "node:assert/strict";
import { test } from "node:test";

import { covers, placeOverlay } from "../../src/js/overlays.js";

test("an overlay below the edit row never covers it, at any size", () => {
  for (const anchor of ["below", "top", "bottom", "corner"]) {
    for (let rows = 1; rows <= 24; rows++) {
      for (let editRow = 0; editRow < rows; editRow++) {
        for (let height = 1; height <= 30; height++) {
          const placement = placeOverlay({ rows, editRow, height, anchor });
          const where = `${anchor} rows=${rows} editRow=${editRow} height=${height}`;
          if (placement.hidden) {
            assert.equal(rows, 1, where);
            continue;
          }
          const shifted = editRow - placement.shift;
          assert.ok(shifted >= 0, `edit row scrolled out of view: ${where}`);
          assert.ok(!covers(placement, shifted), `edit row covered: ${where}`);
          assert.ok(placement.top >= 0 && placement.top + placement.height <= rows, `off screen: ${where}`);
        }
      }
    }
  }
});

test("the menu opens below the line and shifts content up only when short of room", () => {
  assert.deepEqual(placeOverlay({ rows: 24, editRow: 3, height: 5, anchor: "below" }), {
    top: 4,
    height: 5,
    shift: 0,
    edge: "below",
  });
  assert.deepEqual(placeOverlay({ rows: 24, editRow: 22, height: 5, anchor: "below" }), {
    top: 19,
    height: 5,
    shift: 4,
    edge: "below",
  });
});

test("an edge bar moves to the other edge to keep the edit row visible", () => {
  assert.equal(placeOverlay({ rows: 24, editRow: 10, height: 1, anchor: "top" }).edge, "top");
  assert.equal(placeOverlay({ rows: 24, editRow: 0, height: 1, anchor: "top" }).edge, "bottom");
  assert.equal(placeOverlay({ rows: 24, editRow: 23, height: 1, anchor: "bottom" }).edge, "top");
});

test("a corner badge sits bottom right unless the edit row is last", () => {
  assert.equal(placeOverlay({ rows: 24, editRow: 5, anchor: "corner" }).top, 23);
  assert.equal(placeOverlay({ rows: 24, editRow: 23, anchor: "corner" }).top, 0);
});

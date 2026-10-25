import assert from "node:assert/strict";
import { test } from "node:test";

import { TabModel, tabBarVisible } from "../../src/js/tabs.js";

const titles = (model) => model.tabs.map((tab) => tab.title);

test("new tabs go at the end and become active", () => {
  const model = new TabModel();
  const a = model.add({ title: "a" });
  model.add({ title: "b" });
  model.activate(a.id);
  const c = model.add({ title: "c" });
  assert.deepEqual(titles(model), ["a", "b", "c"]);
  assert.equal(model.activeId, c.id);
});

test("closing the active tab activates its right neighbour, or the left one at the end", () => {
  const model = new TabModel();
  const [a, b, c] = ["a", "b", "c"].map((title) => model.add({ title }));
  model.activate(b.id);
  model.close(b.id);
  assert.equal(model.activeId, c.id);
  model.close(c.id);
  assert.equal(model.activeId, a.id);
  model.close(a.id);
  assert.equal(model.activeId, null);
  assert.equal(model.count, 0);
});

test("closing an inactive tab keeps the active one", () => {
  const model = new TabModel();
  const a = model.add({ title: "a" });
  const b = model.add({ title: "b" });
  model.close(a.id);
  assert.equal(model.activeId, b.id);
  assert.equal(model.close(a.id), null, "closing twice is a no-op");
});

test("cycling wraps around and selecting by position ignores missing tabs", () => {
  const model = new TabModel();
  const [a, , c] = ["a", "b", "c"].map((title) => model.add({ title }));
  model.cycle(1);
  assert.equal(model.activeId, a.id);
  model.cycle(-1);
  assert.equal(model.activeId, c.id);
  assert.equal(model.activateIndex(8), false);
  assert.equal(model.activeId, c.id);
});

test("moving clamps to the ends and reports whether anything moved", () => {
  const model = new TabModel();
  const [a] = ["a", "b", "c"].map((title) => model.add({ title }));
  assert.equal(model.move(a.id, 99), true);
  assert.deepEqual(titles(model), ["b", "c", "a"]);
  assert.equal(model.move(a.id, 5), false);
  model.move(a.id, -3);
  assert.deepEqual(titles(model), ["a", "b", "c"]);
});

test("the tab bar is hidden with one tab unless always shown", () => {
  assert.equal(tabBarVisible(1, false), false);
  assert.equal(tabBarVisible(2, false), true);
  assert.equal(tabBarVisible(1, true), true);
});

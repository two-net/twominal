import assert from "node:assert/strict";
import { test } from "node:test";

import { createActionRouter } from "../../src/js/actions.js";
import { sheetGroups } from "../../src/js/dialogs.js";
import { indicatorItems } from "../../src/js/indicators.js";
import { cssColor } from "../../src/js/terminal-view.js";

test("indicators appear only for reported states", () => {
  assert.deepEqual(indicatorItems({ vimMode: null, private: false, scl: null }), []);
  assert.deepEqual(
    indicatorItems({ vimMode: "normal", private: true, scl: "unavailable" }).map((item) => item.label),
    ["Normal", "Private", "Plain terminal"],
  );
  assert.deepEqual(indicatorItems({ scl: "on" }), [], "an active smart command line needs no indicator");
});

const binding = (id, label, group, display) => ({ id, label, group, display });
const selectTabs = (display) =>
  Array.from({ length: 9 }, (_, i) => binding(`select-tab-${i + 1}`, `Go to Tab ${i + 1}`, "Tabs", display(i + 1)));

test("the shortcuts sheet groups bound shortcuts and collapses default tab numbers", () => {
  const groups = sheetGroups([
    binding("new-tab", "New Tab", "Tabs", "⌘T"),
    binding("move-tab-left", "Move Tab Left", "Tabs", null),
    ...selectTabs((n) => `⌘${n}`),
    binding("copy", "Copy", "Terminal", "⌘C"),
  ]);
  assert.deepEqual(groups, [
    {
      name: "Tabs",
      rows: [
        { label: "New Tab", keys: "⌘T" },
        { label: "Go to Tab 1–9", keys: "⌘1 … ⌘9" },
      ],
    },
    { name: "Terminal", rows: [{ label: "Copy", keys: "⌘C" }] },
  ]);
});

test("rebound tab numbers are listed one by one", () => {
  const [group] = sheetGroups(selectTabs((n) => `⌃${n}`));
  assert.equal(group.rows.length, 9);
  assert.equal(group.rows[0].keys, "⌃1");
});

test("actions route by id, with tab numbers parsed", () => {
  const calls = [];
  const run = createActionRouter({
    "new-tab": () => calls.push("new-tab"),
    selectTab: (index) => calls.push(`tab ${index}`),
  });
  run("new-tab");
  run("select-tab-3");
  run("toString");
  assert.deepEqual(calls, ["new-tab", "tab 2"]);
});

test("colours map to theme variables, the 256-colour palette and RGB", () => {
  assert.equal(cssColor(-1), "var(--term-fg)");
  assert.equal(cssColor(-2), "var(--term-bg)");
  assert.equal(cssColor(9), "var(--ansi-9)");
  assert.equal(cssColor(16), "#000000");
  assert.equal(cssColor(196), "#ff0000");
  assert.equal(cssColor(244), "#808080");
  assert.equal(cssColor(0x1000000 | 0x0a0b0c), "#0a0b0c");
});

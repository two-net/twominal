// Dialogs: the Keyboard Shortcuts sheet (UI-06) and the welcome tour (UI-07), whose content
// is in index.html. The sheet lists the effective bindings, the same ones the menu uses, so the
// user's changes appear there too.

/**
 * Groups the bound shortcuts for display. Go to Tab 1–9 collapses into one row while all nine
 * keep their defaults.
 *
 * @param {{id: string, label: string, group: string, display: string|null}[]} bindings
 * @returns {{name: string, rows: {label: string, keys: string}[]}[]}
 */
export function sheetGroups(bindings) {
  const bound = bindings.filter((binding) => binding.display);
  const selectTabs = bound.filter((binding) => /^select-tab-\d$/.test(binding.id));
  const collapse = selectTabs.length === 9 && selectTabs.every((binding, i) => binding.display === `⌘${i + 1}`);

  const groups = new Map();
  for (const binding of bound) {
    let row = { label: binding.label, keys: binding.display };
    if (collapse && binding.id.startsWith("select-tab-")) {
      if (binding.id !== "select-tab-1") continue;
      row = { label: "Go to Tab 1–9", keys: "⌘1 … ⌘9" };
    }
    if (!groups.has(binding.group)) groups.set(binding.group, []);
    groups.get(binding.group).push(row);
  }
  return [...groups].map(([name, rows]) => ({ name, rows }));
}

/** Fills and opens the sheet; resolves when it closes. */
export function openShortcutsSheet(dialog, bindings) {
  const sections = sheetGroups(bindings).flatMap(({ name, rows }) => {
    const heading = document.createElement("h3");
    heading.textContent = name;
    const list = document.createElement("dl");
    for (const { label, keys } of rows) {
      const term = document.createElement("dt");
      term.textContent = label;
      const detail = document.createElement("dd");
      const kbd = document.createElement("kbd");
      kbd.textContent = keys;
      detail.append(kbd);
      list.append(term, detail);
    }
    return [heading, list];
  });
  dialog.querySelector(".shortcut-groups").replaceChildren(...sections);
  return openDialog(dialog);
}

/** Opens a modal dialog unless it is already open; resolves when it closes. */
export function openDialog(dialog) {
  if (dialog.open) return Promise.resolve();
  return new Promise((resolve) => {
    dialog.addEventListener("close", () => resolve(), { once: true });
    dialog.showModal();
  });
}

// Every action, from the menu or the tab bar, is decided here: one place maps an action id to
// what the window does.

/**
 * @param {Record<string, () => void> & { selectTab(index: number): void }} handlers
 * @returns {(id: string) => void}
 */
export function createActionRouter(handlers) {
  return function runAction(id) {
    const selectTab = /^select-tab-([1-9])$/.exec(id);
    if (selectTab) return handlers.selectTab(Number(selectTab[1]) - 1);
    const handler = Object.hasOwn(handlers, id) ? handlers[id] : null;
    if (handler) handler();
    else console.warn(`Unknown action: ${id}`);
  };
}

// App and UI for one window: tabs, input routing, titles, overlays and dialogs.

import { createActionRouter } from "./actions.js";
import { openDialog, openShortcutsSheet } from "./dialogs.js";
import { Indicators } from "./indicators.js";
import { Channel, currentWindow, invoke, listenAll, listenHere } from "./ipc.js";
import { encodeKey } from "./keys.js";
import { OverlayHost } from "./overlays.js";
import { TabBar, TabModel, tabBarVisible } from "./tabs.js";
import { MODE, TerminalView, fitGrid, measureCell } from "./terminal-view.js";

const appWindow = currentWindow();
const init = await invoke("window_init");
let settings = init.settings;

const area = document.getElementById("terminal-area");
const views = document.getElementById("views");
const input = document.getElementById("input");
const shortcutsDialog = document.getElementById("shortcuts");
const tourDialog = document.getElementById("tour");

const cell = measureCell(views);
let grid = fitGrid(views, cell);

const tabs = new TabModel();
const tabBar = new TabBar(document.getElementById("tabbar"), tabs, {
  onSelect: (id) => activate(id),
  onClose: (id) => closeTab(id),
  onNew: () => runAction("new-tab"),
  onMove: (id, index) => moveTab(id, index),
});
const overlays = new OverlayHost(document.getElementById("overlay-layer"), {
  metrics: () => ({ rows: grid.rows, editRow: tabs.active?.view.cursorRow() ?? grid.rows - 1 }),
  setShift: (rows) => tabs.active?.view.setShift(rows),
});
const indicators = new Indicators(overlays);

let resolveFirstFrame;
const firstFrame = new Promise((resolve) => (resolveFirstFrame = resolve));

// Tabs and sessions

async function openTab({ cwd = null, cwdFrom = null } = {}) {
  const previous = tabs.active;
  const tab = tabs.add({
    title: "Twominal",
    windowTitle: "Twominal",
    session: null,
    view: new TerminalView(views),
    // Reported by Vim mode, private mode and the smart command line once they exist (UI-05).
    indicators: { vimMode: null, private: false, scl: null },
  });
  showActive(previous);

  const channel = new Channel();
  channel.onmessage = (event) => onSessionEvent(tab, event);
  const asked = grid;
  try {
    tab.session = await invoke("session_spawn", {
      onEvent: channel,
      cols: grid.cols,
      rows: grid.rows,
      cellWidth: cell.width,
      cellHeight: cell.height,
      cwd,
      cwdFrom,
    });
  } catch (error) {
    tab.view.showFailure(String(error));
    return;
  }
  if (!tabs.get(tab.id)) {
    invoke("session_close", { id: tab.session });
    return;
  }
  if (tabs.activeId !== tab.id) invoke("session_set_visible", { id: tab.session, visible: false });
  if (grid !== asked) resizeSession(tab);
}

function onSessionEvent(tab, event) {
  switch (event.type) {
    case "frame":
      tab.view.applyFrame(event);
      if (tab === tabs.active) {
        overlays.layout();
        placeInput();
      }
      resolveFirstFrame();
      break;
    case "title":
      tab.title = event.tab;
      tab.windowTitle = event.window;
      tabBar.render();
      if (tab === tabs.active) appWindow.setTitle(event.window);
      break;
    case "exited":
      closeTab(tab.id);
      break;
  }
}

/** Brings the window in line with the active tab after it changed from `previous`. */
function showActive(previous = null) {
  const tab = tabs.active;
  if (previous && previous !== tab) {
    previous.view.hide();
    if (previous.session) invoke("session_set_visible", { id: previous.session, visible: false });
  }
  if (tab) {
    tab.view.show();
    if (tab.session && previous !== tab) invoke("session_set_visible", { id: tab.session, visible: true });
    appWindow.setTitle(tab.windowTitle);
    indicators.render(tab.indicators);
    overlays.layout();
    placeInput();
  }
  tabBar.setVisible(tabBarVisible(tabs.count, settings.alwaysShowTabBar));
  tabBar.render();
  input.focus();
}

function activate(id) {
  const previous = tabs.active;
  if (tabs.activate(id)) showActive(previous);
}

function cycleTab(delta) {
  const previous = tabs.active;
  if (tabs.cycle(delta)) showActive(previous);
}

function moveTab(id, index) {
  if (tabs.move(id, index)) tabBar.render({ flip: true });
}

function closeTab(id) {
  const wasActive = tabs.activeId === id;
  const previous = tabs.active;
  const tab = tabs.close(id);
  if (!tab) return;
  tab.view.destroy();
  if (tab.session) invoke("session_close", { id: tab.session });
  if (tabs.count === 0) {
    appWindow.close();
    return;
  }
  showActive(wasActive ? null : previous);
}

function resizeSession(tab) {
  if (!tab.session) return;
  invoke("session_resize", {
    id: tab.session,
    cols: grid.cols,
    rows: grid.rows,
    cellWidth: cell.width,
    cellHeight: cell.height,
  });
}

let resizeTimer;
new ResizeObserver(() => {
  clearTimeout(resizeTimer);
  resizeTimer = setTimeout(() => {
    const next = fitGrid(views, cell);
    if (next.cols === grid.cols && next.rows === grid.rows) return;
    grid = next;
    tabs.tabs.forEach(resizeSession);
    overlays.layout();
  }, 40);
}).observe(views);

// Input: every key goes to the active tab's session; ⌘ shortcuts go to the menu.

/** Puts the input element at the cursor, so input-method windows open there. */
function placeInput() {
  const cursor = tabs.active?.view.cursor;
  const [row, col] = cursor ?? [grid.rows - 1, 0];
  input.style.left = `calc(var(--pad-x) + ${col} * var(--cell-w))`;
  input.style.top = `calc(var(--pad-y) + ${row} * var(--cell-h))`;
}

function send(data) {
  const session = tabs.active?.session;
  if (session && data) invoke("session_input", { id: session, data });
}

// Left Option is Meta and right Option types characters, the TERM-12 default.
const optionDown = { AltLeft: false, AltRight: false };

input.addEventListener("keydown", (event) => {
  if (event.code in optionDown) optionDown[event.code] = true;
  if (event.isComposing || event.keyCode === 229) return;
  const view = tabs.active?.view;
  const bytes = encodeKey(event, {
    appCursor: Boolean(view && view.mode & MODE.APP_CURSOR),
    optionAsMeta: optionDown.AltLeft && !optionDown.AltRight,
  });
  if (bytes === null) return;
  event.preventDefault();
  send(bytes);
});

input.addEventListener("keyup", (event) => {
  if (event.code in optionDown) optionDown[event.code] = false;
});

let composing = false;
input.addEventListener("compositionstart", () => (composing = true));
input.addEventListener("compositionend", () => {
  composing = false;
  flushInput();
});
input.addEventListener("input", (event) => {
  if (!composing && !event.isComposing) flushInput();
});

function flushInput() {
  const text = input.value;
  input.value = "";
  send(text);
}

input.addEventListener("paste", (event) => {
  event.preventDefault();
  const text = event.clipboardData?.getData("text/plain");
  const session = tabs.active?.session;
  if (text && session) invoke("session_paste", { id: session, text });
});

// Keys typed while the terminal text is selected still reach the shell.
document.addEventListener(
  "keydown",
  () => {
    if (document.activeElement !== input && !document.querySelector("dialog[open]")) input.focus();
  },
  { capture: true },
);

area.addEventListener("mouseup", () => {
  if (window.getSelection()?.isCollapsed !== false) input.focus();
});

let wheelLines = 0;
area.addEventListener(
  "wheel",
  (event) => {
    const tab = tabs.active;
    if (!tab?.session) return;
    event.preventDefault();
    wheelLines += event.deltaMode === WheelEvent.DOM_DELTA_LINE ? event.deltaY : event.deltaY / cell.height;
    const lines = Math.trunc(wheelLines);
    if (lines === 0) return;
    wheelLines -= lines;
    if (tab.view.mode & MODE.ALT_SCREEN) {
      // Full-screen programs such as less and Vim scroll with arrow keys.
      const key = lines < 0 ? "ArrowUp" : "ArrowDown";
      send(encodeKey({ key, code: key }, { appCursor: Boolean(tab.view.mode & MODE.APP_CURSOR) }).repeat(Math.abs(lines)));
    } else {
      invoke("session_scroll", { id: tab.session, delta: -lines });
    }
  },
  { passive: false },
);

function setWindowFocused(focused) {
  document.body.classList.toggle("window-focused", focused);
  if (!focused) optionDown.AltLeft = optionDown.AltRight = false;
}
window.addEventListener("focus", () => setWindowFocused(true));
window.addEventListener("blur", () => setWindowFocused(false));
setWindowFocused(document.hasFocus());

for (const dialog of [shortcutsDialog, tourDialog]) dialog.addEventListener("close", () => input.focus());

// Actions

const runAction = createActionRouter({
  "new-tab": () => openTab({ cwdFrom: tabs.active?.session ?? null }),
  "new-window": () => invoke("window_open", { cwdFrom: tabs.active?.session ?? null }),
  "close-tab": () => tabs.active && closeTab(tabs.activeId),
  "close-window": () => appWindow.close(),
  "prev-tab": () => cycleTab(-1),
  "next-tab": () => cycleTab(1),
  selectTab: (index) => tabs.tabs[index] && activate(tabs.tabs[index].id),
  "move-tab-left": () => tabs.active && moveTab(tabs.activeId, tabs.indexOf(tabs.activeId) - 1),
  "move-tab-right": () => tabs.active && moveTab(tabs.activeId, tabs.indexOf(tabs.activeId) + 1),
  "keyboard-shortcuts": () => openShortcutsSheet(shortcutsDialog, init.keymap),
  "welcome-tour": () => openDialog(tourDialog),
  "show-test-overlay": () => toggleTestOverlay(),
});

listenHere("menu-action", (event) => runAction(event.payload));
listenAll("settings-changed", (event) => {
  settings = event.payload;
  tabBar.setVisible(tabBarVisible(tabs.count, settings.alwaysShowTabBar));
});

// A stand-in completion menu for checking overlay placement in debug builds (UI-04).
const TEST_ITEMS = [
  ["checkout", "Switch branches or restore files"],
  ["cherry-pick", "Apply the changes of existing commits"],
  ["clean", "Remove untracked files"],
  ["clone", "Clone a repository into a new directory"],
  ["commit", "Record changes to the repository"],
  ["config", "Get and set repository or global options"],
];

function toggleTestOverlay() {
  if (overlays.has("test")) {
    overlays.hide("test");
    return;
  }
  const list = document.createElement("ul");
  list.className = "test-overlay";
  for (const [name, description] of TEST_ITEMS) {
    const item = document.createElement("li");
    const label = document.createElement("span");
    label.textContent = name;
    const detail = document.createElement("span");
    detail.className = "description";
    detail.textContent = description;
    item.append(label, detail);
    list.append(item);
  }
  // One extra row covers the menu's padding and border.
  overlays.show("test", list, { anchor: "below", height: TEST_ITEMS.length + 1 });
}

// Start

openTab({ cwd: init.initialCwd });

// Shown after the splash screen once it exists (SPL); until then, after the first frame.
if (init.tourPending) {
  firstFrame.then(() => openDialog(tourDialog)).then(() => invoke("tour_mark_seen"));
}

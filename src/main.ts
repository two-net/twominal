import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { Config } from "./config";
import { FONT_CHOICES, defaultConfig, normalizeTheme } from "./config";
import { formatCwd, setHomeGuess } from "./editor";
import { History } from "./history";
import { initPtyEvents, ptyKill } from "./pty";
import { Session, inferHome } from "./session";
import { applyChrome, nextTheme, resolveTheme, themeStatusLabel } from "./theme";
import {
  TAB_MIME,
  closeCurrentWindow,
  emitTabMerge,
  isTabDrag,
  isTauri,
  moveCurrentWindowToCursor,
  openDetachedWindow,
  parseTabDrag,
  windowAtCursor,
  windowCount,
  windowLabel,
} from "./windows";
import "@xterm/xterm/css/xterm.css";

type ShellInfo = { path: string; name: string };

const splash = document.getElementById("splash")!;
const app = document.getElementById("app")!;
const tabbarEl = document.getElementById("tabbar")!;
const tabsEl = document.getElementById("tabs")!;
const sessionsEl = document.getElementById("sessions")!;
const settingsEl = document.getElementById("settings")!;
const fontSelect = document.getElementById("set-font") as HTMLSelectElement;
const shellSelect = document.getElementById("set-shell") as HTMLSelectElement;
const modePill = document.getElementById("mode-pill")!;
const statCwd = document.getElementById("stat-cwd")!;
const statTheme = document.getElementById("stat-theme")!;
const statShell = document.getElementById("stat-shell")!;
const statClock = document.getElementById("stat-clock")!;

let cfg: Config = defaultConfig();
const history = new History();
const sessions: Session[] = [];
let active = 0;
let commands: string[] = [];
let gPending = false;
let defaultShellPath = "";
let shells: ShellInfo[] = [];
let tabDragLocal = false;
let tabDragging = false;
const attaching = new Set<string>();
const attachId = new URLSearchParams(location.search).get("attach");
// Kill this PTY on close only if attach never claimed it. Handing the tab to
// another window parks the session; closing must not kill that live PTY.
let attachOrphan = attachId;

async function loadConfig(): Promise<void> {
  try {
    const loaded = await invoke<Config>("config_get");
    cfg = { ...defaultConfig(), ...loaded, theme: normalizeTheme(loaded.theme) };
  } catch {
    cfg = defaultConfig();
  }
}

function saveConfig(): void {
  void invoke("config_set", { cfg });
}

function applyAll(): void {
  applyChrome(cfg, resolveTheme(cfg));
  for (const s of sessions) s.applyConfig(cfg);
  updateStatus();
}

function fillSettings(): void {
  (document.getElementById("set-theme") as HTMLSelectElement).value = cfg.theme;
  fontSelect.value = cfg.fontFamily;
  (document.getElementById("set-size") as HTMLInputElement).value = String(cfg.fontSize);
  (document.getElementById("set-liga") as HTMLInputElement).checked = cfg.ligatures;
  (document.getElementById("set-vim") as HTMLInputElement).checked = cfg.vimMode;
  (document.getElementById("set-motion") as HTMLInputElement).checked = cfg.reduceMotion;
  (document.getElementById("set-raw") as HTMLInputElement).checked = cfg.rawPty;
  fillShellSelect();
  (document.getElementById("set-abbrev") as HTMLTextAreaElement).value = Object.entries(cfg.abbreviations)
    .map(([k, v]) => `${k}=${v}`)
    .join("\n");
}

function fillShellSelect(): void {
  shellSelect.replaceChildren();
  const defLabel = defaultShellPath ? `Default login shell (${defaultShellPath})` : "Default login shell";
  shellSelect.append(new Option(defLabel, ""));
  for (const s of shells) {
    shellSelect.append(new Option(`${s.name} — ${s.path}`, s.path));
  }
  if (cfg.shell && ![...shellSelect.options].some((o) => o.value === cfg.shell)) {
    shellSelect.append(new Option(cfg.shell, cfg.shell));
  }
  shellSelect.value = cfg.shell ?? "";
}

function readSettings(): void {
  cfg.theme = normalizeTheme((document.getElementById("set-theme") as HTMLSelectElement).value);
  cfg.fontFamily = fontSelect.value;
  cfg.fontSize = Number((document.getElementById("set-size") as HTMLInputElement).value) || 13.5;
  cfg.ligatures = (document.getElementById("set-liga") as HTMLInputElement).checked;
  const shell = shellSelect.value.trim();
  cfg.shell = shell || null;
  cfg.vimMode = (document.getElementById("set-vim") as HTMLInputElement).checked;
  cfg.reduceMotion = (document.getElementById("set-motion") as HTMLInputElement).checked;
  cfg.rawPty = (document.getElementById("set-raw") as HTMLInputElement).checked;
  const abbrev: Record<string, string> = {};
  for (const line of (document.getElementById("set-abbrev") as HTMLTextAreaElement).value.split("\n")) {
    const i = line.indexOf("=");
    if (i <= 0) continue;
    abbrev[line.slice(0, i).trim()] = line.slice(i + 1).trim();
  }
  cfg.abbreviations = abbrev;
  saveConfig();
  applyAll();
}

function shellBasename(): string {
  const path = cfg.shell || defaultShellPath;
  const base = path.replace(/\\/g, "/").split("/").filter(Boolean).pop() ?? path;
  return base.replace(/\.exe$/i, "") || "shell";
}

function updateStatus(): void {
  const s = sessions[active];
  statTheme.textContent = themeStatusLabel(cfg);
  statShell.textContent = shellBasename();
  if (!s) {
    modePill.textContent = "insert";
    modePill.className = "pill ins";
    statCwd.textContent = "";
    return;
  }
  if (s.isPassthrough()) {
    const label = s.altScreen && s.title ? `raw · ${s.title}` : s.raw ? "raw" : s.busy ? "running" : "raw";
    modePill.textContent = label;
    modePill.className = "pill raw";
  } else if (s.editor.mode === "normal") {
    modePill.textContent = "normal";
    modePill.className = "pill nor";
  } else if (s.editor.mode === "visual") {
    modePill.textContent = "visual";
    modePill.className = "pill vis";
  } else {
    modePill.textContent = "insert";
    modePill.className = "pill ins";
  }
  statCwd.textContent = formatCwd(s.cwd);
}

function tickClock(): void {
  statClock.textContent = new Date().toLocaleTimeString();
}

function activate(i: number): void {
  if (!sessions.length) return;
  active = (i + sessions.length) % sessions.length;
  sessions.forEach((s, n) => (n === active ? s.activate() : s.deactivate()));
  updateStatus();
}

function toggleDrawer(open?: boolean): void {
  const next = open ?? !settingsEl.classList.contains("open");
  settingsEl.classList.toggle("open", next);
  requestAnimationFrame(() => sessions[active]?.relayout());
}

function bindSessionChrome(s: Session): void {
  s.onChange = () => {
    if (s.cwd) {
      const home = inferHome(s.cwd);
      if (home) setHomeGuess(home);
    }
    if (sessions[active] === s) updateStatus();
  };
  s.tabEl.addEventListener("click", (e) => {
    const t = e.target as HTMLElement;
    if (t.classList.contains("tab-close")) {
      e.stopPropagation();
      void closeTab(sessions.indexOf(s));
      return;
    }
    activate(sessions.indexOf(s));
  });
  bindDrag(s);
}

function mountSession(s: Session): void {
  bindSessionChrome(s);
  sessions.push(s);
  sessionsEl.append(s.root);
  tabsEl.append(s.tabEl);
}

async function newTab(cwd?: string): Promise<void> {
  const inherit = cwd ?? sessions[active]?.cwd ?? null;
  const s = new Session(cfg, history, commands, inherit);
  mountSession(s);
  activate(sessions.length - 1);
  try {
    await s.start();
  } catch (err) {
    s.tabEl.querySelector(".tab-title")!.textContent = "error";
    console.error(err);
  }
}

async function attachTab(id: string): Promise<void> {
  if (!id || sessions.some((s) => s.id === id) || attaching.has(id)) return;
  attaching.add(id);
  const s = new Session(cfg, history, commands, null);
  mountSession(s);
  activate(sessions.length - 1);
  try {
    await s.attach(id);
    releaseAttachOrphan(id);
  } catch (err) {
    s.tabEl.querySelector(".tab-title")!.textContent = "error";
    console.error(err);
  } finally {
    attaching.delete(id);
  }
}

function releaseAttachOrphan(id: string | null | undefined): void {
  if (id && attachOrphan === id) attachOrphan = null;
}

async function closeTab(i: number): Promise<void> {
  const s = sessions[i];
  if (!s) return;
  await s.dispose({ kill: true });
  sessions.splice(i, 1);
  if (!sessions.length) {
    if (isTauri() && (await windowCount()) > 1) {
      await closeCurrentWindow();
      return;
    }
    await newTab();
    return;
  }
  activate(Math.min(i, sessions.length - 1));
}

function bindDrag(s: Session): void {
  s.tabEl.addEventListener("dragstart", (e) => {
    if (!s.id || (e.target as HTMLElement).classList.contains("tab-close")) {
      e.preventDefault();
      return;
    }
    tabDragLocal = false;
    tabDragging = true;
    const payload = JSON.stringify({ id: s.id, from: windowLabel() });
    e.dataTransfer?.setData(TAB_MIME, payload);
    e.dataTransfer?.setData("text/plain", payload);
    if (e.dataTransfer) e.dataTransfer.effectAllowed = "move";
    s.tabEl.classList.add("dragging");
  });
  s.tabEl.addEventListener("dragend", () => {
    tabDragging = false;
    s.tabEl.classList.remove("dragging");
    tabbarEl.classList.remove("tab-drop");
    void finishTabDrag(s);
  });
  s.tabEl.addEventListener("dragover", (e) => {
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = "move";
  });
  s.tabEl.addEventListener("drop", (e) => {
    e.preventDefault();
    e.stopPropagation();
    const drag = parseTabDrag(e.dataTransfer);
    if (!drag || (drag.from && drag.from !== windowLabel())) return;
    tabDragLocal = true;
    const from = sessions.findIndex((x) => x.id === drag.id);
    const to = sessions.indexOf(s);
    if (from < 0 || from === to) return;
    const [moved] = sessions.splice(from, 1);
    sessions.splice(to, 0, moved);
    tabsEl.replaceChildren(...sessions.map((x) => x.tabEl));
    activate(to);
  });
}

async function finishTabDrag(s: Session): Promise<void> {
  if (tabDragLocal) {
    tabDragLocal = false;
    return;
  }
  if (!isTauri() || !s.id || sessions.indexOf(s) < 0) return;
  let hit: string | null = null;
  try {
    hit = await windowAtCursor();
  } catch {
    return;
  }
  const me = windowLabel();
  if (hit === me) return;
  if (hit) {
    await transferAway(s, () => emitTabMerge(hit!, s.id));
    return;
  }
  if (sessions.length === 1) {
    try {
      await moveCurrentWindowToCursor();
    } catch {
      /* ignore */
    }
    return;
  }
  await transferAway(s, () => openDetachedWindow(s.id));
}

async function transferAway(s: Session, handoff: () => Promise<void>): Promise<void> {
  const id = s.id;
  releaseAttachOrphan(id);
  try {
    await s.parkAndTeardown();
  } catch (err) {
    console.error(err);
    return;
  }
  const i = sessions.indexOf(s);
  if (i >= 0) sessions.splice(i, 1);
  if (sessions.length) activate(Math.min(i < 0 ? sessions.length - 1 : i, sessions.length - 1));
  try {
    await handoff();
  } catch (err) {
    console.error(err);
    await attachTab(id);
    return;
  }
  if (!sessions.length) {
    await closeCurrentWindow();
  }
}

function isActiveTabDrag(dt: DataTransfer | null): boolean {
  return tabDragging || isTabDrag(dt);
}

function bindTabbarDrop(): void {
  tabbarEl.addEventListener("dragover", (e) => {
    if (!isActiveTabDrag(e.dataTransfer)) return;
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = "move";
    tabbarEl.classList.add("tab-drop");
  });
  tabbarEl.addEventListener("dragleave", (e) => {
    if (!tabbarEl.contains(e.relatedTarget as Node)) tabbarEl.classList.remove("tab-drop");
  });
  tabbarEl.addEventListener("drop", (e) => {
    e.preventDefault();
    tabbarEl.classList.remove("tab-drop");
    const drag = parseTabDrag(e.dataTransfer);
    if (!drag || (drag.from && drag.from !== windowLabel())) return;
    tabDragLocal = true;
    const from = sessions.findIndex((x) => x.id === drag.id);
    if (from < 0) return;
    const [moved] = sessions.splice(from, 1);
    sessions.push(moved);
    tabsEl.replaceChildren(...sessions.map((x) => x.tabEl));
    activate(sessions.length - 1);
  });
  // Same-window drop on the terminal must not paste the tab payload or detach.
  app.addEventListener(
    "dragover",
    (e) => {
      if (!isActiveTabDrag(e.dataTransfer)) return;
      e.preventDefault();
      if (e.dataTransfer) e.dataTransfer.dropEffect = "move";
    },
    true,
  );
  app.addEventListener(
    "drop",
    (e) => {
      const drag = parseTabDrag(e.dataTransfer);
      if (!drag?.from) return;
      e.preventDefault();
      if (drag.from === windowLabel()) tabDragLocal = true;
    },
    true,
  );
}

function showAppNow(): void {
  splash.hidden = true;
  splash.setAttribute("aria-hidden", "true");
  app.hidden = false;
  app.classList.add("show");
}

function showSplashThenApp(): Promise<void> {
  const reduce =
    cfg.reduceMotion || window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  if (reduce) {
    showAppNow();
    return Promise.resolve();
  }
  return new Promise((resolve) => {
    window.setTimeout(() => {
      splash.classList.add("hide");
      app.hidden = false;
      app.classList.add("show");
      window.setTimeout(() => {
        splash.hidden = true;
        splash.setAttribute("aria-hidden", "true");
        resolve();
      }, 560);
    }, 1600);
  });
}

function cycleTheme(): void {
  cfg.theme = nextTheme(cfg.theme);
  (document.getElementById("set-theme") as HTMLSelectElement).value = cfg.theme;
  saveConfig();
  applyAll();
}

function onGlobalKeys(e: KeyboardEvent): void {
  const mod = e.metaKey || e.ctrlKey;
  if (mod && e.key === "," && !e.shiftKey) {
    e.preventDefault();
    toggleDrawer();
    return;
  }
  if (mod && e.key.toLowerCase() === "t") {
    e.preventDefault();
    void newTab();
    return;
  }
  if (mod && e.key.toLowerCase() === "w") {
    e.preventDefault();
    void closeTab(active);
    return;
  }
  if (e.key === "Tab" && e.ctrlKey) {
    e.preventDefault();
    activate(active + (e.shiftKey ? -1 : 1));
    return;
  }
  if (mod && e.key >= "1" && e.key <= "9") {
    e.preventDefault();
    activate(Number(e.key) - 1);
    return;
  }
  const s = sessions[active];
  if (cfg.vimMode && s && !s.isPassthrough() && s.editor.mode === "normal" && !e.metaKey && !e.ctrlKey) {
    if (e.key === "g") {
      if (gPending) {
        gPending = false;
        return;
      }
      gPending = true;
      window.setTimeout(() => {
        gPending = false;
      }, 400);
      return;
    }
    if (gPending && e.key === "t") {
      e.preventDefault();
      gPending = false;
      activate(active + 1);
      return;
    }
    if (gPending && e.key === "T") {
      e.preventDefault();
      gPending = false;
      activate(active - 1);
      return;
    }
  }
}

async function main(): Promise<void> {
  for (const f of FONT_CHOICES) {
    const o = document.createElement("option");
    o.value = f;
    o.textContent = f;
    fontSelect.append(o);
  }
  await loadConfig();
  applyChrome(cfg, resolveTheme(cfg));
  await initPtyEvents();
  await history.load();
  try {
    commands = await invoke<string[]>("list_commands");
  } catch {
    commands = [];
  }
  try {
    defaultShellPath = await invoke<string>("default_shell");
  } catch {
    defaultShellPath = "";
  }
  try {
    shells = await invoke<ShellInfo[]>("list_shells");
  } catch {
    shells = [];
  }
  fillSettings();

  document.getElementById("new-tab")!.addEventListener("click", () => void newTab());
  document.getElementById("settings-btn")!.addEventListener("click", () => toggleDrawer());
  document.getElementById("theme-btn")!.addEventListener("click", () => cycleTheme());
  settingsEl.addEventListener("change", () => readSettings());
  (document.getElementById("set-abbrev") as HTMLTextAreaElement).addEventListener("change", () => readSettings());
  window.addEventListener("keydown", onGlobalKeys);
  window.matchMedia("(prefers-color-scheme: dark)").addEventListener("change", () => applyAll());
  window.matchMedia("(prefers-reduced-motion: reduce)").addEventListener("change", () => applyAll());
  bindTabbarDrop();
  tickClock();
  window.setInterval(() => {
    tickClock();
  }, 1000);

  if (isTauri()) {
    const win = getCurrentWindow();
    await listen<{ id: string }>(
      "tab-merge",
      (ev) => {
        if (ev.payload?.id) void attachTab(ev.payload.id);
      },
      { target: windowLabel() },
    );
    await win.onCloseRequested(async () => {
      const leftover = sessions.splice(0);
      await Promise.all(leftover.map((s) => s.dispose({ kill: true })));
      if (attachOrphan && leftover.every((s) => s.id !== attachOrphan)) {
        try {
          await ptyKill(attachOrphan);
        } catch {
          /* already gone */
        }
      }
    });
  }

  if (attachId) {
    showAppNow();
    await attachTab(attachId);
  } else {
    await showSplashThenApp();
    await newTab();
  }
}

void main();

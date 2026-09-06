import { invoke } from "@tauri-apps/api/core";
import { FitAddon } from "@xterm/addon-fit";
import { SerializeAddon } from "@xterm/addon-serialize";
import { Terminal } from "@xterm/xterm";
import type { Config } from "./config";
import { LineEditor, type EditorMode } from "./editor";
import type { History } from "./history";
import { ligatureRanges } from "./ligatures";
import { StreamFilter } from "./osc";
import {
  dropPtyHandlers,
  onPtyExit,
  onPtyOutput,
  ptyKill,
  ptyPark,
  ptyResize,
  ptySpawn,
  ptyUnpark,
  ptyWrite,
} from "./pty";
import { applyChrome, darkXterm, lightXterm, resolveTheme } from "./theme";

export type SessionSnap = {
  title: string;
  cwd: string;
  branch: string | null;
  term: string;
  cols: number;
  rows: number;
  editor: {
    buffer: string;
    cursor: number;
    mode: EditorMode;
    visualAnchor: number;
    yank: string;
  };
  altScreen: boolean;
  appCursor: boolean;
  busy: boolean;
  raw: boolean;
  exited: boolean;
};

// DECSC + rmkx + leave alt-screen before the idle OSC. `dotnet` (and others)
// send smkx and never restore it. DECSC stops 1049l from homing to 0,0 when
// we were never on the alt screen (xterm.js restores savedX/Y, which default
// to the origin).
const IDLE_PRINTF =
  "printf '\\0337\\033[?1049l\\033[?1l\\033>\\033]733;twominal;idle;%s\\007' \"$PWD\"";

const INIT =
  `stty isig -echo 2>/dev/null; { unsetopt zle; unsetopt prompt_cr; unsetopt prompt_sp; set +o emacs; set +o vi; PROMPT=; RPROMPT=; PS1=; PS2=; PROMPT_COMMAND=; precmd_functions=(); preexec_functions=(); chpwd_functions=(); } >/dev/null 2>&1; printf '\\033[2J\\033[3J\\033[H'; ${IDLE_PRINTF}\r`;

const WIPE = "\x1b[2J\x1b[3J\x1b[H";
const VIEW_WIPE = "\x1b[2J\x1b[H";
const RESTORE = "\x1b7\x1b[?1049l\x1b[?1l\x1b>";
const INIT_HOLD_MS = 5000;

export class Session {
  id = "";
  title = "shell";
  cwd = "";
  readonly root: HTMLElement;
  readonly tabEl: HTMLButtonElement;
  private wrap: HTMLElement;
  readonly editor: LineEditor;
  private term: Terminal;
  private fit = new FitAddon();
  private serializeAddon = new SerializeAddon();
  private ligaId: number | null = null;
  private filter = new StreamFilter();
  private observer: ResizeObserver | null = null;
  altScreen = false;
  appCursor = false;
  busy = false;
  raw: boolean;
  exited = false;
  private initSent = false;
  private initCleared = false;
  private startWiped = false;
  private initHold = "";
  private initHoldTimer = 0;
  private lastPass: boolean | null = null;
  private posix = !navigator.userAgent.includes("Windows");
  private onWinKey = (e: KeyboardEvent): void => this.passthroughKey(e);
  onChange: () => void = () => {};

  constructor(
    private cfg: Config,
    history: History,
    commands: string[],
    cwd: string | null,
  ) {
    this.raw = cfg.rawPty;
    this.busy = !cfg.rawPty;
    this.cwd = cwd ?? "";
    this.root = document.createElement("div");
    this.root.className = "session";
    this.wrap = document.createElement("div");
    this.wrap.className = "xterm-wrap";
    this.editor = new LineEditor(history);
    this.editor.vimEnabled = cfg.vimMode;
    this.editor.reduceMotion = cfg.reduceMotion;
    this.editor.abbrevs = { ...cfg.abbreviations };
    this.editor.commandList = commands;
    this.editor.commands = new Set(commands);
    this.editor.onSubmit = (line) => void this.submit(line);
    this.editor.onInterrupt = () => void ptyWrite(this.id, "\x03");
    this.editor.onEof = () => void ptyWrite(this.id, "\x04");
    this.editor.onClear = () => void ptyWrite(this.id, "\x0c");
    this.editor.onChange = () => this.onChange();
    this.root.append(this.wrap, this.editor.root);
    this.wrap.addEventListener("mousedown", () => {
      if (this.isPassthrough()) this.focusTerm();
      else this.editor.focus();
    });

    this.tabEl = document.createElement("button");
    this.tabEl.type = "button";
    this.tabEl.className = "tab";
    this.tabEl.draggable = true;
    this.tabEl.innerHTML = `<span class="tab-title">shell</span><span class="tab-close" title="Close">×</span>`;

    const theme = resolveTheme(cfg);
    this.term = new Terminal({
      cursorBlink: true,
      fontFamily: `"${cfg.fontFamily}", "JetBrains Mono", "Fira Code", ui-monospace, monospace`,
      fontSize: cfg.fontSize,
      fontWeight: "400",
      theme: theme === "dark" ? darkXterm : lightXterm,
      allowProposedApi: true,
      scrollback: 5000,
      macOptionIsMeta: true,
    });
    this.term.loadAddon(this.fit);
    this.term.loadAddon(this.serializeAddon);
    this.term.onData((data) => {
      if (this.isPassthrough()) void ptyWrite(this.id, data);
    });
    this.term.onTitleChange((t) => {
      if (this.altScreen && t) this.setTitle(t);
    });
    this.term.onBell(() => {
      this.tabEl.classList.add("bell");
      setTimeout(() => this.tabEl.classList.remove("bell"), 400);
    });
    this.term.attachCustomKeyEventHandler((ev) => this.termKeys(ev));
  }

  async start(): Promise<void> {
    this.term.open(this.wrap);
    this.fit.fit();
    this.enableLigatures();
    this.refreshWhenFontsReady();
    const cols = this.term.cols || 80;
    const rows = this.term.rows || 24;
    this.id = await ptySpawn(cols, rows, this.cfg.shell, this.cwd || null);
    this.bindPty();
    this.watchSize();
    window.addEventListener("keydown", this.onWinKey, true);
    this.syncChrome();
    if (!this.raw) {
      window.setTimeout(() => this.maybeInit(), 350);
      this.initHoldTimer = window.setTimeout(() => {
        if (this.initCleared) return;
        this.initCleared = true;
        this.releaseInit(true);
      }, INIT_HOLD_MS);
    } else {
      this.term.focus();
    }
  }

  async attach(id: string): Promise<void> {
    this.id = id;
    this.initSent = true;
    this.initCleared = true;
    this.startWiped = true;
    const queued: Uint8Array[] = [];
    let restoring = true;
    onPtyOutput(this.id, (bytes) => {
      if (restoring) queued.push(bytes);
      else this.onBytes(bytes);
    });
    onPtyExit(this.id, () => this.onPtyGone());
    const parked = await ptyUnpark(this.id);
    let snap: SessionSnap | null = null;
    try {
      snap = parked.snapshot ? (JSON.parse(parked.snapshot) as SessionSnap) : null;
    } catch {
      snap = null;
    }
    if (snap) this.applySnap(snap);
    this.term.open(this.wrap);
    if (snap?.cols && snap?.rows) {
      try {
        this.term.resize(snap.cols, snap.rows);
      } catch {
        /* size is best-effort before fit */
      }
    }
    if (snap?.term) {
      await new Promise<void>((resolve) => {
        const timer = window.setTimeout(resolve, 400);
        try {
          this.term.write(snap!.term, () => {
            window.clearTimeout(timer);
            resolve();
          });
        } catch {
          window.clearTimeout(timer);
          resolve();
        }
      });
    }
    this.fit.fit();
    this.enableLigatures();
    this.refreshWhenFontsReady();
    if (parked.backlog.length) this.onBytes(parked.backlog);
    restoring = false;
    for (const chunk of queued) this.onBytes(chunk);
    this.watchSize();
    window.addEventListener("keydown", this.onWinKey, true);
    this.lastPass = null;
    this.syncChrome();
    this.resize();
    if (parked.exited) this.onPtyGone();
  }

  async parkAndTeardown(): Promise<void> {
    if (!this.id) return;
    let term = "";
    try {
      term = this.serializeAddon.serialize();
    } catch {
      term = "";
    }
    const snap: SessionSnap = {
      title: this.title,
      cwd: this.cwd,
      branch: this.editor.branch,
      term,
      cols: this.term.cols || 80,
      rows: this.term.rows || 24,
      editor: {
        buffer: this.editor.buffer,
        cursor: this.editor.cursor,
        mode: this.editor.mode,
        visualAnchor: this.editor.visualAnchor,
        yank: this.editor.yank,
      },
      altScreen: this.altScreen,
      appCursor: this.appCursor,
      busy: this.busy,
      raw: this.raw,
      exited: this.exited,
    };
    await ptyPark(this.id, JSON.stringify(snap));
    await this.dispose({ kill: false });
  }

  private applySnap(snap: SessionSnap): void {
    this.title = snap.title || this.title;
    this.cwd = snap.cwd || this.cwd;
    this.altScreen = !!snap.altScreen;
    this.appCursor = !!snap.appCursor;
    this.busy = !!snap.busy;
    this.raw = !!snap.raw;
    this.exited = !!snap.exited;
    this.editor.buffer = snap.editor?.buffer ?? "";
    this.editor.cursor = snap.editor?.cursor ?? 0;
    this.editor.mode = snap.editor?.mode ?? "insert";
    this.editor.visualAnchor = snap.editor?.visualAnchor ?? 0;
    this.editor.yank = snap.editor?.yank ?? "";
    this.editor.setMeta(this.cwd, snap.branch ?? null);
    this.editor.render();
    const label = this.tabEl.querySelector(".tab-title");
    if (label) label.textContent = this.title;
  }

  private bindPty(): void {
    onPtyOutput(this.id, (bytes) => this.onBytes(bytes));
    onPtyExit(this.id, () => this.onPtyGone());
  }

  private onPtyGone(): void {
    this.exited = true;
    this.busy = false;
    this.syncChrome();
    if (!this.title.endsWith(" (exited)")) this.setTitle(`${this.title} (exited)`);
  }

  private watchSize(): void {
    this.observer = new ResizeObserver(() => this.resize());
    this.observer.observe(this.wrap);
  }

  private maybeInit(): void {
    if (this.raw || !this.posix || this.initSent || !this.id) return;
    this.initSent = true;
    void ptyWrite(this.id, INIT);
  }

  private releaseInit(flush: boolean): void {
    if (this.initHoldTimer) {
      window.clearTimeout(this.initHoldTimer);
      this.initHoldTimer = 0;
    }
    const held = this.initHold;
    this.initHold = "";
    if (flush && held) this.term.write(held);
  }

  private refreshWhenFontsReady(): void {
    void document.fonts.ready.then(() => {
      try {
        this.term.refresh(0, this.term.rows - 1);
      } catch {
        /* disposed before the webfont finished loading */
      }
    });
  }

  private enableLigatures(): void {
    if (this.ligaId !== null) {
      this.term.deregisterCharacterJoiner(this.ligaId);
      this.ligaId = null;
    }
    if (!this.cfg.ligatures) return;
    this.ligaId = this.term.registerCharacterJoiner(ligatureRanges);
  }

  applyConfig(cfg: Config): void {
    this.cfg = cfg;
    const wasRaw = this.raw;
    this.raw = cfg.rawPty;
    if (this.raw && !wasRaw) {
      this.initCleared = true;
      this.releaseInit(true);
    }
    this.editor.vimEnabled = cfg.vimMode;
    if (!cfg.vimMode) this.editor.mode = "insert";
    this.editor.reduceMotion = cfg.reduceMotion;
    this.editor.abbrevs = { ...cfg.abbreviations };
    const theme = resolveTheme(cfg);
    this.term.options.theme = theme === "dark" ? darkXterm : lightXterm;
    this.term.options.fontFamily = `"${cfg.fontFamily}", "JetBrains Mono", "Fira Code", ui-monospace, monospace`;
    this.term.options.fontSize = cfg.fontSize;
    this.enableLigatures();
    this.refreshWhenFontsReady();
    this.fit.fit();
    this.resize();
    this.syncChrome();
    this.editor.render();
  }

  isPassthrough(): boolean {
    // DECCKM (`appCursor`) is ignored: `dotnet --version` leaves it on.
    return this.raw || this.altScreen || this.busy || this.exited;
  }

  relayout(): void {
    this.fit.fit();
    this.resize();
  }

  activate(): void {
    this.root.classList.add("active");
    this.tabEl.classList.add("active");
    requestAnimationFrame(() => {
      this.fit.fit();
      this.resize();
      if (this.isPassthrough()) this.focusTerm();
      else this.editor.focus();
    });
  }

  deactivate(): void {
    this.root.classList.remove("active");
    this.tabEl.classList.remove("active");
    this.editor.menu.hide();
  }

  async dispose(opts: { kill?: boolean } = {}): Promise<void> {
    const kill = opts.kill !== false;
    this.releaseInit(false);
    this.observer?.disconnect();
    this.observer = null;
    window.removeEventListener("keydown", this.onWinKey, true);
    dropPtyHandlers(this.id);
    if (kill && this.id) await ptyKill(this.id);
    this.term.dispose();
    this.root.remove();
    this.tabEl.remove();
  }

  private resize(): void {
    try {
      this.fit.fit();
    } catch {
      return;
    }
    if (this.id) void ptyResize(this.id, this.term.cols, this.term.rows);
  }

  private onBytes(bytes: Uint8Array): void {
    this.maybeInit();
    const { text, events } = this.filter.push(bytes);
    let idleCwd: string | undefined;
    for (const ev of events) {
      if (ev.type === "alt") this.altScreen = ev.on;
      if (ev.type === "appCursor") this.appCursor = ev.on;
      if (ev.type === "idle") idleCwd = ev.cwd;
    }
    const apply = (): void => {
      if (idleCwd !== undefined) void this.onIdle(idleCwd);
      else this.syncChrome();
    };

    // Swallow the injected INIT line (and the login prompt) until idle OSC.
    if (!this.raw && !this.initCleared) {
      if (idleCwd !== undefined) {
        this.initCleared = true;
        this.releaseInit(false);
        apply();
        return;
      }
      if (text) this.initHold += text;
      if (this.altScreen) {
        this.initCleared = true;
        this.releaseInit(true);
        this.syncChrome();
      }
      return;
    }

    if (text) this.term.write(text, apply);
    else apply();
  }

  private async onIdle(cwd: string): Promise<void> {
    this.busy = false;
    this.appCursor = false;
    this.altScreen = false;
    if (!this.raw && !this.startWiped) {
      this.startWiped = true;
      this.initCleared = true;
      this.releaseInit(false);
      // In-band wipe: xterm.clear() is a no-op after 1049l restores cursor to 0,0.
      this.term.write(RESTORE + WIPE);
    } else {
      this.term.write(RESTORE);
    }
    if (cwd) this.cwd = cwd;
    this.setTitle(basename(this.cwd) || "shell");
    let branch: string | null = null;
    try {
      branch = await invoke<string | null>("git_branch", { cwd: this.cwd });
    } catch {
      branch = null;
    }
    this.editor.setMeta(this.cwd, branch);
    this.syncChrome();
    if (!this.isPassthrough()) this.editor.focus();
    this.onChange();
  }

  private async submit(line: string): Promise<void> {
    this.busy = true;
    this.syncChrome();
    this.focusTerm();
    if (!this.raw && line.length) {
      // Idle 1049l restores cursor without erasing. Wipe the viewport so a
      // new command (ssh MOTD especially) does not paint over leftover cells.
      await new Promise<void>((resolve) => {
        try {
          this.term.write(VIEW_WIPE, () => resolve());
        } catch {
          resolve();
        }
      });
    }
    const payload = this.posix && !this.raw ? wrapHybrid(line) : `${line}\r`;
    await ptyWrite(this.id, payload);
    this.focusTerm();
    this.term.scrollToBottom();
  }

  private syncChrome(): void {
    const pass = this.isPassthrough();
    this.root.classList.toggle("passthrough", pass);
    this.editor.captureKeys = !pass;
    this.editor.root.tabIndex = pass ? -1 : 0;
    this.editor.root.toggleAttribute("inert", pass);
    this.term.options.disableStdin = !pass;
    if (this.lastPass !== pass) {
      this.lastPass = pass;
      if (pass) {
        this.term.options.cursorBlink = true;
        this.term.write("\x1b[?25h");
        this.editor.menu.hide();
        this.editor.root.blur();
        this.focusTerm();
      } else {
        this.term.options.cursorBlink = false;
        this.term.write("\x1b[?25l");
        this.editor.focus();
      }
    }
    applyChrome(this.cfg, resolveTheme(this.cfg));
    this.onChange();
  }

  private focusTerm(): void {
    this.term.focus();
    requestAnimationFrame(() => {
      if (this.isPassthrough()) this.term.focus();
    });
  }

  /**
   * Keys often land on the hidden line editor or <body> after Enter (WebKit
   * blurs display:none and may refuse xterm's off-screen textarea). Forward
   * those keystrokes to the PTY so ssh/repls echo what you type.
   */
  private passthroughKey(e: KeyboardEvent): void {
    if (!this.root.classList.contains("active")) return;
    if (!this.isPassthrough()) return;
    if (e.defaultPrevented || e.isComposing) return;
    const t = e.target as HTMLElement | null;
    if (t?.closest(".drawer, .tabbar")) return;
    if (this.term.textarea && t === this.term.textarea) return;
    this.focusTerm();
    const seq = encodeKey(e);
    if (seq === null) return;
    e.preventDefault();
    e.stopPropagation();
    void ptyWrite(this.id, seq);
  }

  private setTitle(title: string): void {
    this.title = title;
    const label = this.tabEl.querySelector(".tab-title");
    if (label) label.textContent = title;
    this.onChange();
  }

  private termKeys(ev: KeyboardEvent): boolean {
    if (ev.type !== "keydown") return true;
    const copy = (ev.metaKey && ev.key === "c") || (ev.ctrlKey && ev.shiftKey && ev.key.toLowerCase() === "c");
    if (copy && this.term.hasSelection()) {
      void navigator.clipboard.writeText(this.term.getSelection());
      return false;
    }
    const paste = (ev.metaKey && ev.key === "v") || (ev.ctrlKey && ev.shiftKey && ev.key.toLowerCase() === "v");
    if (paste && this.isPassthrough()) {
      void navigator.clipboard.readText().then((t) => ptyWrite(this.id, t));
      return false;
    }
    return true;
  }
}

function shSingleQuote(s: string): string {
  return `'${s.replace(/'/g, `'\\''`)}'`;
}

function wrapHybrid(line: string): string {
  const body = line.length ? line : ":";
  const quoted = shSingleQuote(body);
  // In-band wipe stays ordered with command output on the PTY stream.
  const wipe = line.length ? "printf '\\033[2J\\033[H'; " : "";
  // Hybrid idle uses `stty -echo` so the wrap script itself is not printed.
  // Restore cooked echo for the child (ssh, python, cat, nested shells);
  // programs that want raw mode (ssh, vim) still tcsetattr themselves.
  const tty = "stty echo icanon isig 2>/dev/null; ";
  // zsh aborts the rest of a `;` list on SIGINT, so `cmd ; idle` never
  // reports idle after Ctrl+C (ssh password prompts, sleep, …) and the UI
  // stays stuck in RUNNING. An INT trap still emits the idle OSC.
  return (
    `${wipe}` +
    `${tty}` +
    `__tw_done=0; ` +
    `__twominal_idle(){ stty -echo 2>/dev/null; ${IDLE_PRINTF}; }; ` +
    `trap '[ "$__tw_done" = 1 ] || { __tw_done=1; trap - INT; __twominal_idle; }' INT; ` +
    `eval ${quoted} || :; ` +
    `[ "$__tw_done" = 1 ] || { __tw_done=1; trap - INT; __twominal_idle; };\r`
  );
}

function encodeKey(e: KeyboardEvent): string | null {
  if (e.metaKey) return null;
  if (e.key === "Dead" || e.key === "Shift" || e.key === "Control" || e.key === "Alt" || e.key === "Meta") {
    return null;
  }
  if (e.ctrlKey && !e.altKey) {
    const k = e.key.length === 1 ? e.key.toLowerCase() : "";
    if (k.length === 1 && k >= "a" && k <= "z") return String.fromCharCode(k.charCodeAt(0) - 96);
    return null;
  }
  switch (e.key) {
    case "Enter":
      return "\r";
    case "Backspace":
      return "\x7f";
    case "Tab":
      return "\t";
    case "Escape":
      return "\x1b";
    case "Delete":
      return "\x1b[3~";
    case "Home":
      return "\x1b[H";
    case "End":
      return "\x1b[F";
    case "PageUp":
      return "\x1b[5~";
    case "PageDown":
      return "\x1b[6~";
    case "ArrowUp":
      return "\x1b[A";
    case "ArrowDown":
      return "\x1b[B";
    case "ArrowRight":
      return "\x1b[C";
    case "ArrowLeft":
      return "\x1b[D";
    default:
      break;
  }
  if (e.key.length === 1) return e.altKey ? `\x1b${e.key}` : e.key;
  return null;
}

function basename(p: string): string {
  const parts = p.replace(/\\/g, "/").split("/").filter(Boolean);
  return parts[parts.length - 1] || p;
}

export function inferHome(cwd: string): string | null {
  const unix = cwd.match(/^(\/Users\/[^/]+)/) || cwd.match(/^(\/home\/[^/]+)/);
  if (unix) return unix[1];
  const win = cwd.match(/^([A-Za-z]:\\Users\\[^\\]+)/);
  return win ? win[1] : null;
}

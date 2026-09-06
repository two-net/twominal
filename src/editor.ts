import { applyCompletion, complete, needsPathSuggest, suggestPath, suggestSync } from "./complete";
import { CompleteMenu } from "./complete-menu";
import { tokenize, tokenAt, type Token } from "./highlight";
import type { History } from "./history";

export type EditorMode = "insert" | "normal" | "visual";

export class LineEditor {
  readonly root: HTMLElement;
  private promptEl: HTMLElement;
  private lineEl: HTMLElement;
  readonly menu: CompleteMenu;
  buffer = "";
  cursor = 0;
  mode: EditorMode = "insert";
  visualAnchor = 0;
  yank = "";
  vimEnabled = true;
  reduceMotion = false;
  commands = new Set<string>();
  commandList: string[] = [];
  abbrevs: Record<string, string> = {};
  cwd = "";
  branch: string | null = null;
  history: History;
  onSubmit: (line: string) => void = () => {};
  onInterrupt: () => void = () => {};
  onEof: () => void = () => {};
  onClear: () => void = () => {};
  onChange: () => void = () => {};
  /** When false, keys pass through (hidden editor must not swallow SSH/TUI input). */
  captureKeys = true;
  private pathGhost: string | null = null;
  private pathGhostKey = "";
  private pathGhostGen = 0;
  private pathGhostTimer = 0;

  constructor(history: History) {
    this.history = history;
    this.root = el("div", "line-editor");
    this.root.tabIndex = 0;
    this.promptEl = el("div", "prompt");
    this.lineEl = el("div", "line");
    this.menu = new CompleteMenu();
    this.menu.onPick = (item) => this.applyMatch(item);
    this.root.append(this.promptEl, this.lineEl, this.menu.el);
    this.root.addEventListener("keydown", (e) => this.onKey(e));
    this.root.addEventListener("paste", (e) => {
      if (!this.captureKeys) return;
      e.preventDefault();
      const text = e.clipboardData?.getData("text") ?? "";
      this.insert(text.replace(/\r\n/g, "\n").replace(/\r/g, "\n").split("\n")[0] ?? "");
    });
    this.render();
  }

  focus(): void {
    this.root.focus();
  }

  setMeta(cwd: string, branch: string | null): void {
    const cwdChanged = cwd !== this.cwd;
    this.cwd = cwd;
    this.branch = branch;
    this.renderPrompt();
    if (cwdChanged) {
      this.pathGhost = null;
      this.pathGhostKey = "";
      if (this.buffer) this.render();
    }
  }

  setLine(text: string, cursor = text.length): void {
    this.buffer = text;
    this.cursor = Math.max(0, Math.min(text.length, cursor));
    this.render();
  }

  clear(): void {
    this.buffer = "";
    this.cursor = 0;
    this.mode = "insert";
    this.menu.hide();
    this.render();
  }

  private renderPrompt(): void {
    const path = formatCwd(this.cwd);
    this.promptEl.replaceChildren();
    const p = el("span", "cwd");
    p.textContent = path || "~";
    this.promptEl.append(p);
    const mark = el("span", "mark");
    mark.textContent = " ›";
    this.promptEl.append(mark);
    if (this.branch) {
      const g = el("span", "git");
      g.textContent = this.branch;
      this.promptEl.append(g);
    }
  }

  private suggestion(): string | null {
    if (this.cursor !== this.buffer.length || !this.buffer) return null;
    const hist = this.history.suggest(this.buffer);
    if (hist) return hist;
    const sync = suggestSync(this.buffer, this.commandList, this.abbrevs);
    if (sync) return sync;
    if (
      this.pathGhost &&
      this.pathGhost.startsWith(this.buffer) &&
      this.pathGhost.length > this.buffer.length
    ) {
      return this.pathGhost;
    }
    return null;
  }

  render(): void {
    this.renderPrompt();
    const tokens = tokenize(this.buffer, this.commands);
    const ghost = this.suggestion();
    const sel = this.mode === "visual" ? this.selection() : null;
    this.lineEl.replaceChildren();
    // Keep token text in one node so calt/liga can form. Split only at the
    // caret or visual-selection edges (those must break a ligature anyway).
    let idx = 0;
    let caretPlaced = false;
    const placeCaret = (): void => {
      if (caretPlaced) return;
      this.lineEl.append(caret(this.reduceMotion));
      caretPlaced = true;
    };
    for (const tok of tokens) {
      const start = idx;
      const end = start + tok.text.length;
      const cuts = [0, tok.text.length];
      if (this.cursor > start && this.cursor < end) cuts.push(this.cursor - start);
      if (sel) {
        if (sel[0] > start && sel[0] < end) cuts.push(sel[0] - start);
        if (sel[1] > start && sel[1] < end) cuts.push(sel[1] - start);
      }
      cuts.sort((a, b) => a - b);
      let prev = 0;
      for (const cut of cuts) {
        if (cut === prev) continue;
        if (this.cursor === start + prev) placeCaret();
        const s = el("span", kindClass(tok));
        s.textContent = tok.text.slice(prev, cut);
        if (sel && start + prev >= sel[0] && start + prev < sel[1]) s.classList.add("sel");
        this.lineEl.append(s);
        prev = cut;
      }
      if (this.cursor === end) placeCaret();
      idx = end;
    }
    if (!caretPlaced) placeCaret();
    if (ghost && ghost.length > this.buffer.length) {
      const g = el("span", "ghost");
      g.textContent = ghost.slice(this.buffer.length);
      this.lineEl.append(g);
    }
    this.schedulePathGhost();
    this.onChange();
  }

  private schedulePathGhost(): void {
    const line = this.buffer;
    if (this.cursor !== line.length || !line) {
      window.clearTimeout(this.pathGhostTimer);
      this.pathGhostGen += 1;
      this.pathGhostKey = "";
      return;
    }
    const key = `${this.cwd}\n${line}`;
    if (this.history.suggest(line) || suggestSync(line, this.commandList, this.abbrevs) || !needsPathSuggest(line)) {
      this.pathGhost = null;
      if (this.pathGhostKey !== key) {
        this.pathGhostKey = key;
        window.clearTimeout(this.pathGhostTimer);
        this.pathGhostGen += 1;
      }
      return;
    }
    if (this.pathGhostKey === key) return;
    this.pathGhostKey = key;
    if (this.pathGhost && !this.pathGhost.startsWith(line)) this.pathGhost = null;
    window.clearTimeout(this.pathGhostTimer);
    const gen = ++this.pathGhostGen;
    this.pathGhostTimer = window.setTimeout(() => {
      void this.loadPathGhost(line, this.cwd, gen);
    }, 30);
  }

  private async loadPathGhost(line: string, cwd: string, gen: number): Promise<void> {
    const ghost = await suggestPath(line, cwd);
    if (gen !== this.pathGhostGen) return;
    this.pathGhost = ghost;
    if (this.buffer === line && this.cwd === cwd) this.render();
  }

  private selection(): [number, number] {
    const a = Math.min(this.visualAnchor, this.cursor);
    const b = Math.max(this.visualAnchor, this.cursor);
    return [a, b === a ? a + 1 : b];
  }

  private onKey(e: KeyboardEvent): void {
    if (!this.captureKeys) return;
    if (this.menu.open) {
      if (e.key === "ArrowDown") {
        e.preventDefault();
        this.menu.move(1);
        return;
      }
      if (e.key === "ArrowUp") {
        e.preventDefault();
        this.menu.move(-1);
        return;
      }
      if (e.key === "Enter" || e.key === "Tab") {
        e.preventDefault();
        this.menu.pick();
        return;
      }
      if (e.key === "Escape") {
        e.preventDefault();
        this.menu.hide();
        return;
      }
      if (e.key.startsWith("Arrow") || e.key === "Home" || e.key === "End") {
        e.preventDefault();
        return;
      }
    }
    if (e.key === "Tab") {
      e.preventDefault();
      void this.doComplete();
      return;
    }
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "c" && !e.shiftKey) {
      if (this.buffer.length) {
        e.preventDefault();
        this.clear();
      } else {
        e.preventDefault();
        this.onInterrupt();
      }
      return;
    }
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "d") {
      if (!this.buffer) {
        e.preventDefault();
        this.onEof();
      }
      return;
    }
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "l") {
      e.preventDefault();
      this.onClear();
      return;
    }
    if (e.key === "Enter") {
      e.preventDefault();
      this.menu.hide();
      const line = this.buffer;
      this.history.add(line);
      this.clear();
      this.onSubmit(line);
      return;
    }
    if (this.vimEnabled && this.mode !== "insert") {
      if (this.handleVim(e)) {
        e.preventDefault();
        this.render();
      }
      return;
    }
    if (this.vimEnabled && e.key === "Escape") {
      e.preventDefault();
      this.mode = "normal";
      if (this.cursor > 0) this.cursor -= 1;
      this.render();
      return;
    }
    if (e.key === "ArrowRight" || (e.ctrlKey && e.key.toLowerCase() === "f")) {
      const g = this.suggestion();
      if (g && this.cursor === this.buffer.length) {
        e.preventDefault();
        this.buffer = g;
        this.cursor = g.length;
        this.render();
        return;
      }
    }
    if (e.key === " " && !e.metaKey && !e.ctrlKey) {
      const tok = tokenAt(this.buffer, this.cursor);
      const exp = this.abbrevs[tok.text];
      if (exp && this.cursor === tok.end) {
        e.preventDefault();
        this.buffer = this.buffer.slice(0, tok.start) + exp + " " + this.buffer.slice(tok.end);
        this.cursor = tok.start + exp.length + 1;
        this.render();
        return;
      }
    }
    if (e.key === "ArrowUp") {
      e.preventDefault();
      const v = this.history.prev(this.buffer);
      if (v !== null) this.setLine(v);
      return;
    }
    if (e.key === "ArrowDown") {
      e.preventDefault();
      const v = this.history.next();
      if (v !== null) this.setLine(v);
      return;
    }
    if (e.key === "Backspace") {
      e.preventDefault();
      this.menu.hide();
      if (this.cursor > 0) {
        this.buffer = this.buffer.slice(0, this.cursor - 1) + this.buffer.slice(this.cursor);
        this.cursor -= 1;
        this.render();
      }
      return;
    }
    if (e.key === "Delete") {
      e.preventDefault();
      this.menu.hide();
      this.buffer = this.buffer.slice(0, this.cursor) + this.buffer.slice(this.cursor + 1);
      this.render();
      return;
    }
    if (e.key === "ArrowLeft") {
      e.preventDefault();
      this.cursor = Math.max(0, this.cursor - 1);
      this.render();
      return;
    }
    if (e.key === "ArrowRight") {
      e.preventDefault();
      this.cursor = Math.min(this.buffer.length, this.cursor + 1);
      this.render();
      return;
    }
    if (e.key === "Home" || (e.ctrlKey && e.key.toLowerCase() === "a")) {
      e.preventDefault();
      this.cursor = 0;
      this.render();
      return;
    }
    if (e.key === "End" || (e.ctrlKey && e.key.toLowerCase() === "e")) {
      e.preventDefault();
      this.cursor = this.buffer.length;
      this.render();
      return;
    }
    if (e.key.length === 1 && !e.metaKey && !e.ctrlKey && !e.altKey) {
      e.preventDefault();
      this.insert(e.key);
    }
  }

  private insert(text: string): void {
    this.buffer = this.buffer.slice(0, this.cursor) + text + this.buffer.slice(this.cursor);
    this.cursor += text.length;
    this.menu.hide();
    this.render();
  }

  private applyMatch(item: string): void {
    const next = applyCompletion(this.buffer, this.cursor, item);
    this.setLine(next.line, next.cursor);
  }

  private async doComplete(): Promise<void> {
    const items = await complete(this.buffer, this.cursor, this.cwd, this.commandList, this.abbrevs);
    if (!items.length) {
      this.menu.hide();
      return;
    }
    if (items.length === 1) {
      this.menu.hide();
      this.applyMatch(items[0]);
      return;
    }
    this.menu.show(items);
  }

  private handleVim(e: KeyboardEvent): boolean {
    if (e.metaKey || e.ctrlKey || e.altKey) return false;
    if (this.mode === "visual") return this.handleVisual(e);
    switch (e.key) {
      case "h":
        this.cursor = Math.max(0, this.cursor - 1);
        return true;
      case "l":
        this.cursor = Math.min(Math.max(0, this.buffer.length - 1), this.cursor + 1);
        return true;
      case "j": {
        const v = this.history.next();
        if (v !== null) this.setLine(v, 0);
        return true;
      }
      case "k": {
        const v = this.history.prev(this.buffer);
        if (v !== null) this.setLine(v, 0);
        return true;
      }
      case "w":
        this.cursor = nextWord(this.buffer, this.cursor);
        return true;
      case "b":
        this.cursor = prevWord(this.buffer, this.cursor);
        return true;
      case "0":
        this.cursor = 0;
        return true;
      case "$":
        this.cursor = Math.max(0, this.buffer.length - 1);
        return true;
      case "x":
        this.yank = this.buffer[this.cursor] ?? "";
        this.buffer = this.buffer.slice(0, this.cursor) + this.buffer.slice(this.cursor + 1);
        this.cursor = Math.min(this.cursor, Math.max(0, this.buffer.length - 1));
        return true;
      case "d":
        return pendingDd(this);
      case "i":
        this.mode = "insert";
        return true;
      case "a":
        this.mode = "insert";
        this.cursor = Math.min(this.buffer.length, this.cursor + 1);
        return true;
      case "I":
        this.mode = "insert";
        this.cursor = 0;
        return true;
      case "A":
        this.mode = "insert";
        this.cursor = this.buffer.length;
        return true;
      case "v":
        this.mode = "visual";
        this.visualAnchor = this.cursor;
        return true;
      case "p":
        this.insert(this.yank);
        return true;
      case "Escape":
        return true;
      default:
        return false;
    }
  }

  private handleVisual(e: KeyboardEvent): boolean {
    switch (e.key) {
      case "Escape":
        this.mode = "normal";
        return true;
      case "h":
        this.cursor = Math.max(0, this.cursor - 1);
        return true;
      case "l":
        this.cursor = Math.min(this.buffer.length, this.cursor + 1);
        return true;
      case "0":
        this.cursor = 0;
        return true;
      case "$":
        this.cursor = this.buffer.length;
        return true;
      case "y": {
        const [a, b] = this.selection();
        this.yank = this.buffer.slice(a, b);
        this.mode = "normal";
        return true;
      }
      case "d":
      case "x": {
        const [a, b] = this.selection();
        this.yank = this.buffer.slice(a, b);
        this.buffer = this.buffer.slice(0, a) + this.buffer.slice(b);
        this.cursor = a;
        this.mode = "normal";
        return true;
      }
      default:
        return true;
    }
  }
}

let ddArmed = false;
function pendingDd(ed: LineEditor): boolean {
  if (!ddArmed) {
    ddArmed = true;
    setTimeout(() => {
      ddArmed = false;
    }, 500);
    return true;
  }
  ddArmed = false;
  ed.yank = ed.buffer;
  ed.buffer = "";
  ed.cursor = 0;
  return true;
}

function nextWord(s: string, i: number): number {
  let j = i;
  while (j < s.length && /\S/.test(s[j])) j++;
  while (j < s.length && /\s/.test(s[j])) j++;
  return j;
}

function prevWord(s: string, i: number): number {
  let j = Math.max(0, i - 1);
  while (j > 0 && /\s/.test(s[j])) j--;
  while (j > 0 && /\S/.test(s[j - 1])) j--;
  return j;
}

function el(tag: string, className: string): HTMLElement {
  const n = document.createElement(tag);
  n.className = className;
  return n;
}

function caret(reduce: boolean): HTMLElement {
  const c = el("span", reduce ? "caret static" : "caret");
  c.textContent = "";
  return c;
}

function kindClass(tok: Token): string {
  return `tok ${tok.kind}`;
}

let homeGuess: string | null = null;

export function setHomeGuess(home: string | null): void {
  homeGuess = home;
}

export function formatCwd(cwd: string): string {
  let path = cwd || "~";
  if (homeGuess && (path === homeGuess || path.startsWith(homeGuess + "/") || path.startsWith(homeGuess + "\\"))) {
    path = "~" + path.slice(homeGuess.length);
  }
  return path || "~";
}

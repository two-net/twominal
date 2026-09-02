import { invoke } from "@tauri-apps/api/core";
import { escapeHtml } from "../utils/ansi";

export interface CompletionItem {
  label: string;
  value: string;
  kind: "file" | "dir" | "executable" | "builtin" | "history";
  description?: string;
}

export const BUILTIN_COMMANDS: CompletionItem[] = [
  { label: "/help", value: "/help", kind: "builtin", description: "Twominal builtin & slash commands guide" },
  { label: "/theme", value: "/theme", kind: "builtin", description: "Switch theme mode (dark | light | auto)" },
  { label: "/font", value: "/font", kind: "builtin", description: "Configure typography, size, weight & ligatures" },
  { label: "/vim", value: "/vim", kind: "builtin", description: "Toggle Vim modal navigation mode (NORMAL/INSERT)" },
  { label: "/matrix", value: "/matrix", kind: "builtin", description: "Toggle background matrix digital rain effect" },
  { label: "/clear", value: "/clear", kind: "builtin", description: "Clear terminal scrollback buffer" },
  { label: "/neofetch", value: "/neofetch", kind: "builtin", description: "Twominal system & environment info banner" },
  { label: "/settings", value: "/settings", kind: "builtin", description: "Open settings & typography modal" },
  { label: "/stack", value: "/stack", kind: "builtin", description: "Twominal engine architecture & compilation info" },
  { label: "/ligatures", value: "/ligatures", kind: "builtin", description: "Test or toggle font programming ligatures" },
  { label: "/history", value: "/history", kind: "builtin", description: "Display persistent command execution history" },
  { label: "/date", value: "/date", kind: "builtin", description: "Display current system timestamp" },
  { label: "/tabs", value: "/tabs", kind: "builtin", description: "Multi-tab management (new, close, next, prev)" },
  { label: "/pty", value: "/pty", kind: "builtin", description: "Run command in raw interactive PTY mode" },
  { label: "/exit", value: "/exit", kind: "builtin", description: "Close active shell tab session" },
];

export type CompletionSelectCallback = (item: CompletionItem) => void;

export class CompletionMenu {
  private menuEl: HTMLElement;
  private itemsContainerEl: HTMLElement;
  private items: CompletionItem[] = [];
  private selectedIndex: number = 0;
  private isVisible: boolean = false;
  private onSelectCallback: CompletionSelectCallback | null = null;

  constructor() {
    this.menuEl = document.getElementById("completion-menu") as HTMLElement;
    if (!this.menuEl) {
      this.menuEl = document.createElement("div");
      this.menuEl.id = "completion-menu";
      this.menuEl.className = "hidden mt-2 p-2 rounded-lg bg-[#161b22] border border-[#30363d] shadow-2xl z-30 max-w-lg";
      const viewport = document.getElementById("terminal-viewport");
      viewport?.appendChild(this.menuEl);
    }

    let itemsCont = document.getElementById("completion-items");
    if (!itemsCont) {
      itemsCont = document.createElement("div");
      itemsCont.id = "completion-items";
      itemsCont.className = "grid grid-cols-2 sm:grid-cols-3 gap-1 text-xs";
      this.menuEl.appendChild(itemsCont);
    }
    this.itemsContainerEl = itemsCont;
  }

  public filterBuiltinCommands(prefix: string): CompletionItem[] {
    const clean = (prefix || "").toLowerCase().trim();
    if (!clean || clean === "/") {
      return [...BUILTIN_COMMANDS];
    }
    return BUILTIN_COMMANDS.filter((cmd) => cmd.label.toLowerCase().startsWith(clean));
  }

  public showCustomItems(items: CompletionItem[], onSelect: CompletionSelectCallback): void {
    if (!items || items.length === 0) {
      this.hide();
      return;
    }
    this.items = items;
    this.onSelectCallback = onSelect;
    this.selectedIndex = 0;
    this.render();
    this.show();
  }

  public async trigger(
    cwd: string,
    currentInput: string,
    onSelect: CompletionSelectCallback
  ): Promise<boolean> {
    this.onSelectCallback = onSelect;

    const parts = currentInput.split(/\s+/);
    const targetWord = parts[parts.length - 1] || "";

    // If starting with '/', use built-in slash command completions directly
    if (targetWord.startsWith("/") && !targetWord.slice(1).includes("/")) {
      const builtins = this.filterBuiltinCommands(targetWord);
      if (builtins.length > 0) {
        if (builtins.length === 1 && targetWord.toLowerCase() === builtins[0].label.toLowerCase()) {
          onSelect(builtins[0]);
          this.hide();
          return true;
        }
        this.items = builtins;
        this.selectedIndex = 0;
        this.render();
        this.show();
        return true;
      }
    }

    try {
      const results = await invoke<CompletionItem[]>("fish_get_completions", {
        cwd: cwd || ".",
        prefix: targetWord,
      });

      if (!results || results.length === 0) {
        this.hide();
        return false;
      }

      if (results.length === 1) {
        // Single match auto-complete
        onSelect(results[0]);
        this.hide();
        return true;
      }

      this.items = results;
      this.selectedIndex = 0;
      this.render();
      this.show();
      return true;
    } catch (err) {
      console.warn("Failed to fetch completions:", err);
      this.hide();
      return false;
    }
  }

  public getIsOpen(): boolean {
    return this.isVisible;
  }

  public show(): void {
    this.isVisible = true;
    this.menuEl.classList.remove("hidden");
  }

  public hide(): void {
    this.isVisible = false;
    this.menuEl.classList.add("hidden");
    this.items = [];
    this.selectedIndex = 0;
  }

  public selectNext(): void {
    if (this.items.length === 0) return;
    this.selectedIndex = (this.selectedIndex + 1) % this.items.length;
    this.render();
  }

  public selectPrevious(): void {
    if (this.items.length === 0) return;
    this.selectedIndex = (this.selectedIndex - 1 + this.items.length) % this.items.length;
    this.render();
  }

  public getSelectedItem(): CompletionItem | null {
    if (this.items.length === 0 || this.selectedIndex < 0 || this.selectedIndex >= this.items.length) {
      return null;
    }
    return this.items[this.selectedIndex];
  }

  public confirmSelection(): boolean {
    const item = this.getSelectedItem();
    if (item && this.onSelectCallback) {
      this.onSelectCallback(item);
      this.hide();
      return true;
    }
    this.hide();
    return false;
  }

  private render(): void {
    this.itemsContainerEl.innerHTML = "";

    const isBuiltinPalette = this.items.length > 0 && this.items.every(i => i.kind === "builtin" && i.label.startsWith("/"));
    
    if (isBuiltinPalette) {
      this.itemsContainerEl.className = "flex flex-col gap-1 text-xs max-h-64 overflow-y-auto";
    } else {
      this.itemsContainerEl.className = "grid grid-cols-2 sm:grid-cols-3 gap-1 text-xs max-h-60 overflow-y-auto";
    }

    this.items.forEach((cand, idx) => {
      const isSelected = idx === this.selectedIndex;
      const div = document.createElement("div");

      if (isBuiltinPalette) {
        div.className = `px-2.5 py-1.5 rounded-md cursor-pointer font-mono transition-colors text-xs flex items-center justify-between gap-3 ${
          isSelected
            ? "dark:bg-cyan-500/20 bg-blue-50 dark:text-cyan-300 text-blue-700 dark:border-cyan-500/40 border-blue-300 border font-bold"
            : "dark:hover:bg-slate-700/50 hover:bg-slate-100 dark:text-slate-300 text-slate-700 border border-transparent"
        }`;
        div.innerHTML = `
          <div class="flex items-center gap-2 min-w-0">
            <span class="px-1.5 py-0.5 rounded text-[10px] font-bold dark:bg-cyan-900/60 bg-blue-100 dark:text-cyan-300 text-blue-800 border dark:border-cyan-700/50 border-blue-200 flex-shrink-0">/</span>
            <span class="font-bold dark:text-white text-slate-900 truncate">${escapeHtml(cand.label)}</span>
          </div>
          ${cand.description ? `<span class="text-[11px] dark:text-slate-400 text-slate-500 truncate font-sans text-right">${escapeHtml(cand.description)}</span>` : ""}
        `;
      } else {
        div.className = `px-2 py-1 rounded cursor-pointer font-mono truncate transition-colors text-xs flex items-center gap-1.5 ${
          isSelected
            ? "dark:bg-cyan-500/20 bg-blue-50 dark:text-cyan-300 text-blue-700 dark:border-cyan-500/40 border-blue-300 border font-bold"
            : "dark:hover:bg-slate-700/50 hover:bg-slate-100 dark:text-slate-300 text-slate-700 border border-transparent"
        }`;

        let icon = "📄";
        if (cand.kind === "dir") icon = "📁";
        else if (cand.kind === "executable") icon = "⚡";
        else if (cand.kind === "builtin") icon = "⚙️";

        div.innerHTML = `<span>${icon}</span><span class="truncate">${escapeHtml(cand.label)}</span>`;
      }

      div.onclick = () => {
        this.selectedIndex = idx;
        this.confirmSelection();
      };

      this.itemsContainerEl.appendChild(div);

      if (isSelected) {
        div.scrollIntoView({ block: "nearest" });
      }
    });
  }
}


import { ThemeManager } from "../theme/ThemeManager";
import { VimModeEngine } from "../vim/VimModeEngine";
import { FontManager } from "../fonts/FontManager";

export class StatusBar {
  private clockInterval: number | null = null;
  private vimEngine: VimModeEngine;
  private fontManager: FontManager;
  private themeManager: ThemeManager;
  private onOpenSettingsCallback: () => void;

  constructor(
    vimEngine: VimModeEngine,
    fontManager: FontManager,
    themeManager: ThemeManager,
    onOpenSettings: () => void
  ) {
    this.vimEngine = vimEngine;
    this.fontManager = fontManager;
    this.themeManager = themeManager;
    this.onOpenSettingsCallback = onOpenSettings;

    this.initClock();
    this.bindEvents();
    this.update();
  }

  private initClock(): void {
    const clockEl = document.getElementById("sb-clock");
    const update = () => {
      if (clockEl) {
        const d = new Date();
        clockEl.textContent = d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });
      }
    };
    update();
    if (this.clockInterval) clearInterval(this.clockInterval);
    this.clockInterval = window.setInterval(update, 1000);
  }

  private bindEvents(): void {
    document.getElementById("sb-btn-ligatures")?.addEventListener("click", () => {
      this.fontManager.toggleLigatures();
      this.update();
    });

    document.getElementById("sb-btn-font")?.addEventListener("click", () => {
      this.onOpenSettingsCallback();
    });

    this.vimEngine.onModeChange(() => this.update());
    this.fontManager.onFontChange(() => this.update());
    this.themeManager.onThemeChange(() => this.update());
  }

  public update(): void {
    const isVim = this.vimEngine.getIsEnabled();
    const vimMode = this.vimEngine.getMode();

    const badge = document.getElementById("vim-mode-badge");
    const btnVim = document.getElementById("btn-toggle-vim-mode");

    if (badge) {
      badge.textContent = isVim ? `VIM: ${vimMode}` : "VIM: OFF";
    }

    if (btnVim) {
      if (isVim) {
        if (vimMode === "NORMAL") {
          btnVim.className = "flex items-center gap-1.5 px-2 py-0.5 rounded-md dark:bg-amber-500/20 bg-amber-100 dark:text-amber-300 text-amber-900 dark:border-amber-500/40 border-amber-400 border transition-colors shadow-xs cursor-pointer";
        } else if (vimMode === "VISUAL") {
          btnVim.className = "flex items-center gap-1.5 px-2 py-0.5 rounded-md dark:bg-purple-500/20 bg-purple-100 dark:text-purple-300 text-purple-900 dark:border-purple-500/40 border-purple-400 border transition-colors shadow-xs cursor-pointer";
        } else {
          btnVim.className = "flex items-center gap-1.5 px-2 py-0.5 rounded-md dark:bg-cyan-500/20 bg-cyan-100 dark:text-cyan-300 text-cyan-900 dark:border-cyan-500/40 border-cyan-400 border transition-colors shadow-xs cursor-pointer";
        }
      } else {
        btnVim.className = "flex items-center gap-1.5 px-2 py-0.5 rounded-md dark:bg-slate-800 bg-slate-200 dark:text-slate-400 text-slate-600 dark:border-slate-700 border-slate-300 border transition-colors shadow-xs cursor-pointer opacity-75";
      }
    }
  }

  public dispose(): void {
    if (this.clockInterval) {
      clearInterval(this.clockInterval);
      this.clockInterval = null;
    }
  }
}

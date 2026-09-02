import { invoke } from "@tauri-apps/api/core";
import { TabSessionData } from "../terminal/CommandExecutor";
import { escapeHtml } from "../utils/ansi";
import { VimModeEngine } from "../vim/VimModeEngine";

export type TabTitleFormat = "macos" | "folder" | "process" | "numbered" | "path";
export type TabChangeListener = (activeTab: TabSessionData | null, allTabs: TabSessionData[]) => void;

/**
 * Extracts the base directory name from a given path (handles ~, root /, and paths)
 */
export function getDirectoryBasename(cwd: string, displayCwd?: string): string {
  const p = (displayCwd || cwd || "").trim();
  if (!p || p === "~" || p === "." || p === "/") return p || "~";
  if (p === "~") return "~";

  const cleaned = p.replace(/[/\\]+$/, "");
  if (!cleaned || cleaned === "~") return "~";

  const parts = cleaned.split(/[/\\]/);
  return parts[parts.length - 1] || cleaned;
}

/**
 * Formats a tab title according to native macOS terminal conventions
 */
export function computeTabTitle(
  tab: { cwd: string; displayCwd: string; customTitle?: string | null; activeProcess?: string },
  tabIndex: number,
  format: TabTitleFormat = "macos"
): string {
  // If user explicitly renamed this tab, use their custom name
  if (tab.customTitle && tab.customTitle.trim().length > 0) {
    return tab.customTitle.trim();
  }

  const folder = getDirectoryBasename(tab.cwd, tab.displayCwd);
  const process = tab.activeProcess || "fish";
  const displayPath = tab.displayCwd || tab.cwd || "~";

  switch (format) {
    case "macos":
      // Standard native macOS Terminal format: "folder — process" (e.g. "twominal-gemini — fish", "~ — fish")
      return `${folder} — ${process}`;
    case "folder":
      return folder;
    case "process":
      return process;
    case "numbered":
      // "1. folder — process"
      return `${tabIndex + 1}. ${folder} — ${process}`;
    case "path":
      return `${displayPath} — ${process}`;
    default:
      return `${folder} — ${process}`;
  }
}

export class TabManager {
  private tabs: TabSessionData[] = [];
  private activeTabId: string | null = null;
  private tabsListEl: HTMLElement;
  private listeners: Set<TabChangeListener> = new Set();
  private vimEngine?: VimModeEngine;
  private titleFormat: TabTitleFormat = "macos";
  private editingTabId: string | null = null;
  private contextMenuEl: HTMLElement | null = null;
  private dragSession: { cleanup: () => void } | null = null;

  constructor(tabsListEl: HTMLElement, vimEngine?: VimModeEngine) {
    this.tabsListEl = tabsListEl;
    this.vimEngine = vimEngine;

    // Load saved tab title format
    try {
      const savedFormat = localStorage.getItem("twominal_tab_title_format") as TabTitleFormat;
      if (savedFormat && ["macos", "folder", "process", "numbered", "path"].includes(savedFormat)) {
        this.titleFormat = savedFormat;
      }
    } catch {
      // Ignore
    }

    this.setupWindowControls();
    this.setupGlobalClickForContextMenu();
  }

  public getTitleFormat(): TabTitleFormat {
    return this.titleFormat;
  }

  public setTitleFormat(format: TabTitleFormat): void {
    this.titleFormat = format;
    try {
      localStorage.setItem("twominal_tab_title_format", format);
    } catch {
      // Ignore
    }
    this.refreshTabTitles();
  }

  public onTabChange(listener: TabChangeListener): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  private notify(): void {
    const active = this.getActiveTab();
    this.updateDocumentTitle(active);

    for (const listener of this.listeners) {
      try {
        listener(active, [...this.tabs]);
      } catch (err) {
        console.error("TabManager listener error:", err);
      }
    }
  }

  private updateDocumentTitle(activeTab: TabSessionData | null): void {
    if (activeTab) {
      document.title = `${activeTab.title} — Twominal`;
    } else {
      document.title = "Twominal";
    }
  }

  public getTabs(): TabSessionData[] {
    return [...this.tabs];
  }

  public getActiveTab(): TabSessionData | null {
    return this.tabs.find((t) => t.id === this.activeTabId) || this.tabs[0] || null;
  }

  public refreshTabTitles(): void {
    this.tabs.forEach((tab, index) => {
      tab.title = computeTabTitle(tab, index, this.titleFormat);
    });
    this.renderTabs();
    const active = this.getActiveTab();
    this.updateDocumentTitle(active);
    this.notify();
  }

  public renameTab(id: string, newTitle: string | null): void {
    const tab = this.tabs.find((t) => t.id === id);
    if (!tab) return;

    if (newTitle && newTitle.trim().length > 0) {
      tab.customTitle = newTitle.trim();
    } else {
      tab.customTitle = null; // Reset to dynamic macOS title
    }

    this.editingTabId = null;
    this.refreshTabTitles();
  }

  private getDefaultVimMode(): boolean {
    if (this.vimEngine) {
      return this.vimEngine.getIsEnabled();
    }
    try {
      const saved = localStorage.getItem("twominal_vim_mode");
      if (saved === "off") return false;
    } catch {
      // Ignore
    }
    return true;
  }

  public async createNewTab(title?: string, initialCwd?: string): Promise<TabSessionData> {
    const id = "tab-" + Date.now() + "-" + Math.random().toString(36).slice(2, 6);

    let realCwd = initialCwd || "";
    let displayCwd = "~";
    let gitBranch = "";

    // Inherit working directory from current active tab if not specified
    const activeTab = this.getActiveTab();
    if (!realCwd && activeTab) {
      realCwd = activeTab.cwd;
      displayCwd = activeTab.displayCwd;
    }

    if (!realCwd) {
      try {
        const sysInfo = await invoke<any>("get_system_info", { cwd: null });
        if (sysInfo) {
          realCwd = sysInfo.cwd;
          displayCwd = sysInfo.display_cwd || sysInfo.cwd || "~";
        }
      } catch {
        realCwd = "~";
        displayCwd = "~";
      }
    }

    try {
      const branch = await invoke<string | null>("get_git_branch", { cwd: realCwd });
      if (branch) {
        gitBranch = branch;
      }
    } catch {
      // Ignore
    }

    const customTitle = title ? title.trim() : null;
    const tabIndex = this.tabs.length;

    const newTab: TabSessionData = {
      id,
      title: "",
      customTitle,
      activeProcess: "fish",
      cwd: realCwd,
      displayCwd: displayCwd,
      gitBranch: gitBranch,
      history: [
        "echo 'Welcome to Twominal!'",
        "neofetch",
        "ls -la"
      ],
      historyIndex: 3,
      outputHistory: [],
      currentInput: "",
      vimMode: this.getDefaultVimMode(),
      vimState: "INSERT",
      vimCursorPos: 0,
    };

    // Calculate native macOS title (e.g. "twominal-gemini — fish")
    newTab.title = computeTabTitle(newTab, tabIndex, this.titleFormat);

    this.tabs.push(newTab);
    this.renderTabs();
    this.switchTab(id);
    this.notify();

    return newTab;
  }

  public async createTabFromSession(sessionData: Partial<TabSessionData>): Promise<TabSessionData> {
    const id = sessionData.id || ("tab-" + Date.now() + "-" + Math.random().toString(36).slice(2, 6));
    const realCwd = sessionData.cwd || "~";
    const displayCwd = sessionData.displayCwd || "~";
    let gitBranch = sessionData.gitBranch || "";

    if (!gitBranch && realCwd && realCwd !== "~") {
      try {
        const branch = await invoke<string | null>("get_git_branch", { cwd: realCwd });
        if (branch) gitBranch = branch;
      } catch {}
    }

    const tabIndex = this.tabs.length;
    const newTab: TabSessionData = {
      id,
      title: sessionData.title || "",
      customTitle: sessionData.customTitle || null,
      activeProcess: sessionData.activeProcess || "fish",
      cwd: realCwd,
      displayCwd: displayCwd,
      gitBranch: gitBranch,
      history: sessionData.history && sessionData.history.length > 0 ? [...sessionData.history] : [
        "echo 'Welcome to Twominal!'",
        "neofetch",
        "ls -la"
      ],
      historyIndex: sessionData.historyIndex ?? 3,
      outputHistory: sessionData.outputHistory ? [...sessionData.outputHistory] : [],
      currentInput: sessionData.currentInput || "",
      vimMode: sessionData.vimMode ?? this.getDefaultVimMode(),
      vimState: sessionData.vimState || "INSERT",
      vimCursorPos: sessionData.vimCursorPos || 0,
    };

    if (!newTab.title) {
      newTab.title = computeTabTitle(newTab, tabIndex, this.titleFormat);
    }

    this.tabs.push(newTab);
    this.renderTabs();
    this.switchTab(id);
    this.notify();

    return newTab;
  }

  public async detachTabToNewWindow(id: string, screenX?: number, screenY?: number): Promise<void> {
    const tab = this.tabs.find((t) => t.id === id);
    if (!tab) return;

    const newWinLabel = "win-" + Date.now() + "-" + Math.random().toString(36).slice(2, 6);

    const payload: Partial<TabSessionData> & { timestamp: number } = {
      id: "tab-" + Date.now(),
      title: tab.title,
      customTitle: tab.customTitle,
      activeProcess: tab.activeProcess,
      cwd: tab.cwd,
      displayCwd: tab.displayCwd,
      gitBranch: tab.gitBranch,
      history: tab.history ? [...tab.history] : [],
      historyIndex: tab.historyIndex,
      outputHistory: tab.outputHistory ? tab.outputHistory.slice(-50) : [],
      currentInput: tab.currentInput || "",
      vimMode: tab.vimMode,
      vimState: tab.vimState || "INSERT",
      vimCursorPos: tab.vimCursorPos || 0,
      timestamp: Date.now(),
    };

    try {
      localStorage.setItem(`twominal_init_tab_${newWinLabel}`, JSON.stringify(payload));
      localStorage.setItem("twominal_latest_detached_tab", JSON.stringify(payload));
    } catch (e) {
      console.error("Failed to store detached tab payload:", e);
    }

    // If multiple tabs, close tab in current window. If single tab, reset current tab.
    if (this.tabs.length > 1) {
      const idx = this.tabs.findIndex((t) => t.id === id);
      if (idx !== -1) {
        if (tab.terminalSession) {
          invoke("pty_kill", { id: tab.id }).catch(() => {});
          tab.terminalSession.destroy();
        }
        this.tabs.splice(idx, 1);
        if (this.activeTabId === id) {
          const nextActive = this.tabs[Math.max(0, idx - 1)];
          this.activeTabId = nextActive.id;
        }
        this.refreshTabTitles();
      }
    } else {
      // Only 1 tab in this window: reset output and input
      const currentTab = this.tabs[0];
      if (currentTab) {
        currentTab.outputHistory = [];
        currentTab.currentInput = "";
        this.refreshTabTitles();
      }
    }

    let success = false;
    try {
      const posX = typeof screenX === "number" && screenX > 0 ? Math.max(30, Math.round(screenX - 180)) : undefined;
      const posY = typeof screenY === "number" && screenY > 0 ? Math.max(30, Math.round(screenY - 30)) : undefined;

      await invoke("window_create_new", {
        label: newWinLabel,
        title: tab.title || "Twominal",
        x: posX,
        y: posY,
        width: 1000,
        height: 650,
      });
      success = true;
    } catch (err) {
      console.warn("window_create_new failed or in browser mode:", err);
    }

    if (!success) {
      try {
        const left = typeof screenX === "number" && screenX > 0 ? Math.max(30, Math.round(screenX - 180)) : 100;
        const top = typeof screenY === "number" && screenY > 0 ? Math.max(30, Math.round(screenY - 30)) : 100;
        window.open(window.location.href, "_blank", `width=1000,height=650,left=${left},top=${top}`);
      } catch (browserErr) {
        console.error("Browser window.open failed:", browserErr);
      }
    }
  }

  public async createNewWindow(initialCwd?: string): Promise<void> {
    const newWinLabel = "win-" + Date.now() + "-" + Math.random().toString(36).slice(2, 6);
    const realCwd = initialCwd || this.getActiveTab()?.cwd || "~";
    const displayCwd = this.getActiveTab()?.displayCwd || "~";

    const payload: Partial<TabSessionData> & { timestamp: number } = {
      id: "tab-" + Date.now(),
      title: "",
      cwd: realCwd,
      displayCwd: displayCwd,
      gitBranch: "",
      history: ["echo 'Welcome to Twominal!'", "neofetch", "ls -la"],
      historyIndex: 3,
      outputHistory: [],
      currentInput: "",
      vimMode: this.getDefaultVimMode(),
      vimState: "INSERT",
      vimCursorPos: 0,
      timestamp: Date.now(),
    };

    try {
      localStorage.setItem(`twominal_init_tab_${newWinLabel}`, JSON.stringify(payload));
      localStorage.setItem("twominal_latest_detached_tab", JSON.stringify(payload));
    } catch (e) {
      console.error("Failed to store init tab in localStorage:", e);
    }

    let success = false;
    try {
      await invoke("window_create_new", {
        label: newWinLabel,
        title: "Twominal",
        width: 1000,
        height: 650,
      });
      success = true;
    } catch (err) {
      console.warn("window_create_new failed or in browser:", err);
    }

    if (!success) {
      window.open(window.location.href, "_blank", "width=1000,height=650");
    }
  }

  public switchTab(id: string): void {
    this.activeTabId = id;
    this.editingTabId = null;
    this.renderTabs();
    this.notify();
  }

  public closeTab(id: string, e?: MouseEvent): void {
    if (e) e.stopPropagation();

    // Terminate any active process or PTY session in this tab
    invoke("shell_cancel", { tabId: id }).catch(() => {});
    invoke("pty_kill", { id }).catch(() => {});

    const targetTab = this.tabs.find((t) => t.id === id);
    targetTab?.terminalSession?.destroy();

    if (this.tabs.length <= 1) {
      const tab = this.tabs[0];
      if (tab) {
        tab.outputHistory = [];
        tab.currentInput = "";
        this.notify();
      }
      return;
    }

    const index = this.tabs.findIndex((t) => t.id === id);
    if (index === -1) return;

    this.tabs.splice(index, 1);

    if (this.activeTabId === id) {
      const nextActive = this.tabs[Math.max(0, index - 1)];
      this.activeTabId = nextActive.id;
    }

    this.refreshTabTitles();
  }

  public closeOtherTabs(id: string): void {
    for (const tab of this.tabs) {
      if (tab.id !== id) {
        invoke("shell_cancel", { tabId: tab.id }).catch(() => {});
        invoke("pty_kill", { id: tab.id }).catch(() => {});
        tab.terminalSession?.destroy();
      }
    }
    this.tabs = this.tabs.filter((t) => t.id === id);
    this.activeTabId = id;
    this.refreshTabTitles();
  }

  public closeActiveTab(): void {
    if (this.activeTabId) {
      this.closeTab(this.activeTabId);
    }
  }

  public switchToNextTab(): void {
    if (this.tabs.length <= 1) return;
    const currentIndex = this.tabs.findIndex((t) => t.id === this.activeTabId);
    const nextIndex = (currentIndex + 1) % this.tabs.length;
    this.switchTab(this.tabs[nextIndex].id);
  }

  public switchToPrevTab(): void {
    if (this.tabs.length <= 1) return;
    const currentIndex = this.tabs.findIndex((t) => t.id === this.activeTabId);
    const prevIndex = (currentIndex - 1 + this.tabs.length) % this.tabs.length;
    this.switchTab(this.tabs[prevIndex].id);
  }

  public switchToTabIndex(index: number): void {
    if (index >= 0 && index < this.tabs.length) {
      this.switchTab(this.tabs[index].id);
    }
  }

  public startEditingTab(id: string): void {
    this.editingTabId = id;
    this.renderTabs();
  }

  private cancelTabDrag(): void {
    if (this.dragSession) {
      this.dragSession.cleanup();
      this.dragSession = null;
    }
  }

  private initTabDrag(tab: TabSessionData, tabEl: HTMLElement, startEvent: PointerEvent): void {
    this.cancelTabDrag();

    const startX = startEvent.clientX;
    const startY = startEvent.clientY;

    let isDragging = false;
    let isDetachedMode = false;
    let targetIndex = this.tabs.findIndex((t) => t.id === tab.id);

    let ghostEl: HTMLElement | null = null;
    let dropIndicatorEl: HTMLElement | null = null;

    const onPointerMove = (e: PointerEvent) => {
      const deltaX = e.clientX - startX;
      const deltaY = e.clientY - startY;
      const distance = Math.hypot(deltaX, deltaY);

      if (!isDragging) {
        if (distance > 5) {
          isDragging = true;
          if (this.activeTabId !== tab.id) {
            this.switchTab(tab.id);
          }

          tabEl.classList.add("opacity-40", "scale-95");

          ghostEl = document.createElement("div");
          ghostEl.id = "tab-drag-ghost";
          ghostEl.className = "fixed pointer-events-none z-[9999] select-none transition-transform duration-75";
          document.body.appendChild(ghostEl);

          dropIndicatorEl = document.createElement("div");
          dropIndicatorEl.className = "w-1 h-6 bg-cyan-400 rounded-full shadow-[0_0_8px_rgba(34,211,238,0.9)] z-30 transition-all pointer-events-none flex-shrink-0";
        } else {
          return;
        }
      }

      if (!ghostEl) return;

      const tabListRect = this.tabsListEl.getBoundingClientRect();
      const isOutsideVertical = e.clientY > tabListRect.bottom + 15 || e.clientY < tabListRect.top - 15;
      const isOutsideHorizontal = e.clientX < tabListRect.left - 25 || e.clientX > tabListRect.right + 25;
      const isOutsideWindow = e.clientX < 0 || e.clientX > window.innerWidth || e.clientY < 0 || e.clientY > window.innerHeight;

      const shouldDetach = isOutsideVertical || isOutsideHorizontal || isOutsideWindow;

      if (shouldDetach) {
        if (!isDetachedMode) {
          isDetachedMode = true;
          if (dropIndicatorEl?.parentElement) {
            dropIndicatorEl.remove();
          }
        }

        const ghostLeft = Math.min(window.innerWidth - 270, Math.max(10, e.clientX - 50));
        const ghostTop = Math.min(window.innerHeight - 120, Math.max(10, e.clientY + 15));
        ghostEl.style.left = `${ghostLeft}px`;
        ghostEl.style.top = `${ghostTop}px`;
        ghostEl.innerHTML = `
          <div class="flex flex-col gap-1.5 p-3 rounded-xl text-xs font-mono dark:bg-[#161b22]/95 bg-white/95 dark:text-slate-100 text-slate-800 border-2 dark:border-cyan-400 border-blue-500 shadow-[0_12px_36px_rgba(0,0,0,0.45)] backdrop-blur-md min-w-[250px] max-w-[320px] animate-pulse-subtle">
            <div class="flex items-center justify-between pb-1 border-b dark:border-slate-700/60 border-slate-200">
              <div class="flex items-center gap-1.5">
                <span class="w-2.5 h-2.5 rounded-full bg-red-500 inline-block shadow-xs"></span>
                <span class="w-2.5 h-2.5 rounded-full bg-amber-500 inline-block shadow-xs"></span>
                <span class="w-2.5 h-2.5 rounded-full bg-emerald-500 inline-block shadow-xs"></span>
              </div>
              <span class="text-[10px] font-bold dark:text-cyan-300 text-blue-600 flex items-center gap-1">
                <svg class="w-3 h-3" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M10 6H6a2 2 0 00-2 2v10a2 2 0 002 2h10a2 2 0 002-2v-4M14 4h6m0 0v6m0-6L10 14"/></svg>
                Detach to New Window
              </span>
            </div>
            <div class="flex items-center gap-2 mt-0.5">
              <span class="text-xs dark:text-cyan-400 text-blue-600 font-bold flex-shrink-0">➜</span>
              <span class="font-bold truncate text-xs dark:text-white text-slate-900">${escapeHtml(tab.title)}</span>
            </div>
            <div class="text-[10px] dark:text-slate-400 text-slate-500 truncate flex items-center gap-1">
              <span>📁</span>
              <span class="truncate">${escapeHtml(tab.displayCwd || tab.cwd || "~")}</span>
            </div>
            <div class="mt-1 pt-1.5 border-t dark:border-slate-800 border-slate-100 flex items-center justify-between text-[10px]">
              <span class="dark:text-emerald-400 text-emerald-600 font-medium">Release to spawn window</span>
              <span class="text-[9px] dark:text-slate-500 text-slate-400 font-mono">Twominal</span>
            </div>
          </div>
        `;
      } else {
        if (isDetachedMode) {
          isDetachedMode = false;
        }

        ghostEl.style.left = `${e.clientX - 25}px`;
        ghostEl.style.top = `${e.clientY - 15}px`;
        ghostEl.innerHTML = `
          <div class="flex items-center gap-2 px-3 py-1.5 rounded-lg text-xs font-mono dark:bg-[#1c2128] bg-white dark:text-cyan-300 text-blue-600 border dark:border-cyan-500/70 border-blue-400 shadow-xl backdrop-blur-md">
            <span class="text-[11px] font-bold">⇋</span>
            <span class="font-semibold truncate max-w-[180px]">${escapeHtml(tab.title)}</span>
          </div>
        `;

        const tabChildren = Array.from(this.tabsListEl.children).filter(
          (el) => el !== dropIndicatorEl && el.hasAttribute("data-tab-id")
        ) as HTMLElement[];

        let bestIndex = tabChildren.length;
        for (let i = 0; i < tabChildren.length; i++) {
          const rect = tabChildren[i].getBoundingClientRect();
          const midX = rect.left + rect.width / 2;
          if (e.clientX < midX) {
            bestIndex = i;
            break;
          }
        }

        targetIndex = bestIndex;

        if (dropIndicatorEl) {
          if (targetIndex >= tabChildren.length) {
            this.tabsListEl.appendChild(dropIndicatorEl);
          } else {
            this.tabsListEl.insertBefore(dropIndicatorEl, tabChildren[targetIndex]);
          }
        }
      }
    };

    const onPointerUp = async (e: PointerEvent) => {
      cleanup();

      if (!isDragging) {
        if (!this.editingTabId) {
          this.switchTab(tab.id);
        }
        return;
      }

      if (isDetachedMode) {
        const screenX = e.screenX || (window.screenX + e.clientX);
        const screenY = e.screenY || (window.screenY + e.clientY);
        await this.detachTabToNewWindow(tab.id, screenX, screenY);
      } else {
        const currentIndex = this.tabs.findIndex((t) => t.id === tab.id);
        if (currentIndex !== -1 && targetIndex !== currentIndex && targetIndex !== currentIndex + 1) {
          const [moved] = this.tabs.splice(currentIndex, 1);
          const insertAt = targetIndex > currentIndex ? targetIndex - 1 : targetIndex;
          this.tabs.splice(insertAt, 0, moved);
          this.refreshTabTitles();
        } else {
          this.renderTabs();
        }
      }
    };

    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        cleanup();
        this.renderTabs();
      }
    };

    const cleanup = () => {
      window.removeEventListener("pointermove", onPointerMove);
      window.removeEventListener("pointerup", onPointerUp);
      window.removeEventListener("pointercancel", cleanup);
      window.removeEventListener("keydown", onKeyDown);
      tabEl.classList.remove("opacity-40", "scale-95");
      if (ghostEl?.parentElement) ghostEl.remove();
      if (dropIndicatorEl?.parentElement) dropIndicatorEl.remove();
      this.dragSession = null;
    };

    window.addEventListener("pointermove", onPointerMove);
    window.addEventListener("pointerup", onPointerUp);
    window.addEventListener("pointercancel", cleanup);
    window.addEventListener("keydown", onKeyDown);

    this.dragSession = { cleanup };
  }

  public renderTabs(): void {
    this.tabsListEl.innerHTML = "";

    this.tabs.forEach((tab, index) => {
      const isActive = tab.id === this.activeTabId;
      const isEditing = tab.id === this.editingTabId;
      const tabEl = document.createElement("div");

      // Native macOS tab item styling
      tabEl.className = `group relative flex items-center gap-2 px-3 py-1.5 rounded-lg text-xs font-mono cursor-pointer transition-all duration-150 border select-none ${
        isActive
          ? "dark:bg-[#1c2128] bg-white dark:text-slate-100 text-slate-900 dark:border-[#30363d] border-slate-300 shadow-xs font-semibold"
          : "dark:bg-slate-800/30 bg-slate-200/50 dark:text-slate-400 text-slate-600 dark:hover:text-slate-200 hover:text-slate-900 border-transparent dark:hover:bg-slate-800/60 hover:bg-slate-200/90 font-normal"
      }`;

      // Tooltip with macOS terminal info and shortcut key hint
      const tooltip = `${tab.displayCwd || tab.cwd} (${tab.activeProcess || "fish"}) • Drag out for new window • Cmd+${index + 1}`;
      tabEl.setAttribute("title", tooltip);
      tabEl.setAttribute("data-tab-id", tab.id);

      // Pointer drag interaction (handles both click to switch, reordering, and dragging out to detach)
      tabEl.addEventListener("pointerdown", (e: PointerEvent) => {
        if (e.button !== 0) return;
        if (isEditing) return;
        const target = e.target as HTMLElement | null;
        if (target?.closest("[data-close-id]") || target?.closest(".tab-rename-input")) {
          return;
        }
        this.initTabDrag(tab, tabEl, e);
      });

      // Double click to rename tab
      tabEl.ondblclick = (e) => {
        e.stopPropagation();
        this.startEditingTab(tab.id);
      };

      // Right click context menu
      tabEl.oncontextmenu = (e) => {
        e.preventDefault();
        e.stopPropagation();
        this.showContextMenu(e.clientX, e.clientY, tab);
      };

      // Icon: Native macOS terminal prompt chevron
      const iconHtml = `
        <span class="text-[11px] opacity-75 flex items-center flex-shrink-0">
          <svg class="w-3.5 h-3.5 ${isActive ? "dark:text-cyan-400 text-blue-600 font-bold" : "dark:text-slate-500 text-slate-400"}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <path d="m4 17 6-6-6-6M12 19h8"/>
          </svg>
        </span>
      `;

      // Title content (Editable input or static formatted title)
      let titleContentHtml = "";
      if (isEditing) {
        titleContentHtml = `
          <input 
            type="text" 
            class="tab-rename-input px-1.5 py-0.2 rounded dark:bg-slate-800 bg-slate-100 border dark:border-cyan-400 border-blue-500 text-xs font-mono dark:text-slate-100 text-slate-900 outline-none w-36 sm:w-64" 
            value="${escapeHtml(tab.customTitle || tab.title)}" 
            placeholder="${escapeHtml(computeTabTitle(tab, index, this.titleFormat))}"
          />
        `;
      } else {
        titleContentHtml = `
          <span class="tab-title-text truncate max-w-[200px] sm:max-w-[360px] md:max-w-[480px] text-xs font-mono select-none" data-tab-id="${tab.id}">
            ${escapeHtml(tab.title)}
          </span>
        `;
      }

      // macOS round close button
      const closeBtnHtml = `
        <button 
          title="Close Tab (Cmd+W)" 
          class="w-4 h-4 rounded-full flex items-center justify-center text-[10px] dark:text-slate-400 text-slate-500 hover:bg-slate-300/80 dark:hover:bg-slate-700/80 hover:text-red-500 dark:hover:text-red-400 transition-colors flex-shrink-0 opacity-70 group-hover:opacity-100" 
          data-close-id="${tab.id}"
        >✕</button>
      `;

      tabEl.innerHTML = `${iconHtml}${titleContentHtml}${closeBtnHtml}`;

      // Handle close button
      const closeBtn = tabEl.querySelector(`[data-close-id="${tab.id}"]`);
      closeBtn?.addEventListener("click", (e) => {
        e.stopPropagation();
        this.closeTab(tab.id, e as MouseEvent);
      });

      // Handle rename input events
      if (isEditing) {
        const input = tabEl.querySelector(".tab-rename-input") as HTMLInputElement | null;
        if (input) {
          setTimeout(() => {
            input.focus();
            input.select();
          }, 10);

          const commitEdit = () => {
            const val = input.value.trim();
            this.renameTab(tab.id, val.length > 0 ? val : null);
          };

          input.addEventListener("keydown", (e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              commitEdit();
            } else if (e.key === "Escape") {
              e.preventDefault();
              this.editingTabId = null;
              this.renderTabs();
            }
          });

          input.addEventListener("blur", () => {
            commitEdit();
          });

          input.addEventListener("click", (e) => {
            e.stopPropagation();
          });
        }
      }

      this.tabsListEl.appendChild(tabEl);
    });
  }

  private showContextMenu(x: number, y: number, tab: TabSessionData): void {
    this.hideContextMenu();

    const menu = document.createElement("div");
    menu.id = "tab-context-menu";
    menu.className = "fixed z-50 rounded-xl dark:bg-[#1c2128] bg-white dark:border-[#30363d] border-slate-200 border shadow-2xl py-1.5 text-xs font-mono min-w-[190px] animate-fade-in";
    menu.style.left = `${x}px`;
    menu.style.top = `${y}px`;

    menu.innerHTML = `
      <div class="px-3 py-1 text-[10px] font-semibold dark:text-slate-400 text-slate-500 uppercase tracking-wider border-b dark:border-slate-800 border-slate-100">
        Tab Options
      </div>
      <button id="ctx-rename" class="w-full text-left px-3 py-1.5 dark:hover:bg-slate-700/50 hover:bg-slate-100 flex items-center gap-2 dark:text-slate-200 text-slate-700">
        <span>✏️</span> Rename Tab...
      </button>
      ${
        tab.customTitle
          ? `
        <button id="ctx-reset-title" class="w-full text-left px-3 py-1.5 dark:hover:bg-slate-700/50 hover:bg-slate-100 flex items-center gap-2 dark:text-slate-200 text-slate-700">
          <span>🔄</span> Reset to macOS Title
        </button>
      `
          : ""
      }
      <button id="ctx-duplicate" class="w-full text-left px-3 py-1.5 dark:hover:bg-slate-700/50 hover:bg-slate-100 flex items-center gap-2 dark:text-slate-200 text-slate-700">
        <span>📑</span> Duplicate Tab in CWD
      </button>
      <button id="ctx-detach-window" class="w-full text-left px-3 py-1.5 dark:hover:bg-cyan-500/20 hover:bg-blue-50 flex items-center gap-2 dark:text-cyan-300 text-blue-600 font-medium">
        <span>⤢</span> Move Tab to New Window
      </button>
      <div class="border-t dark:border-slate-700/50 border-slate-200 my-1"></div>
      <button id="ctx-close" class="w-full text-left px-3 py-1.5 dark:hover:bg-red-500/20 hover:bg-red-50 flex items-center gap-2 text-red-500">
        <span>✕</span> Close Tab
      </button>
      <button id="ctx-close-others" class="w-full text-left px-3 py-1.5 dark:hover:bg-slate-700/50 hover:bg-slate-100 flex items-center gap-2 dark:text-slate-400 text-slate-500">
        <span>🗑️</span> Close Other Tabs
      </button>
    `;

    document.body.appendChild(menu);
    this.contextMenuEl = menu;

    // Adjust position if overflowing screen
    const rect = menu.getBoundingClientRect();
    if (rect.right > window.innerWidth) {
      menu.style.left = `${window.innerWidth - rect.width - 10}px`;
    }
    if (rect.bottom > window.innerHeight) {
      menu.style.top = `${window.innerHeight - rect.height - 10}px`;
    }

    menu.querySelector("#ctx-rename")?.addEventListener("click", () => {
      this.hideContextMenu();
      this.startEditingTab(tab.id);
    });

    menu.querySelector("#ctx-reset-title")?.addEventListener("click", () => {
      this.hideContextMenu();
      this.renameTab(tab.id, null);
    });

    menu.querySelector("#ctx-duplicate")?.addEventListener("click", () => {
      this.hideContextMenu();
      this.createNewTab(undefined, tab.cwd);
    });

    menu.querySelector("#ctx-detach-window")?.addEventListener("click", () => {
      this.hideContextMenu();
      this.detachTabToNewWindow(tab.id);
    });

    menu.querySelector("#ctx-close")?.addEventListener("click", () => {
      this.hideContextMenu();
      this.closeTab(tab.id);
    });

    menu.querySelector("#ctx-close-others")?.addEventListener("click", () => {
      this.hideContextMenu();
      this.closeOtherTabs(tab.id);
    });
  }

  private hideContextMenu(): void {
    if (this.contextMenuEl) {
      this.contextMenuEl.remove();
      this.contextMenuEl = null;
    }
  }

  private setupGlobalClickForContextMenu(): void {
    window.addEventListener("click", (e) => {
      if (this.contextMenuEl && !this.contextMenuEl.contains(e.target as Node)) {
        this.hideContextMenu();
      }
    });

    window.addEventListener("contextmenu", (e) => {
      const target = e.target as HTMLElement | null;
      if (this.contextMenuEl && target && !this.contextMenuEl.contains(target)) {
        this.hideContextMenu();
      }
    });
  }

  private setupWindowControls(): void {
    document.getElementById("btn-win-close")?.addEventListener("click", () => {
      invoke("window_close").catch(() => {
        this.closeActiveTab();
      });
    });

    document.getElementById("btn-win-min")?.addEventListener("click", () => {
      invoke("window_minimize").catch(() => {
        const win = document.getElementById("window-container");
        win?.classList.toggle("scale-95");
        win?.classList.toggle("opacity-60");
      });
    });

    document.getElementById("btn-win-max")?.addEventListener("click", () => {
      invoke("window_toggle_maximize").catch(() => {
        if (!document.fullscreenElement) {
          document.documentElement.requestFullscreen().catch(() => {});
        } else {
          document.exitFullscreen().catch(() => {});
        }
      });
    });

    document.getElementById("btn-new-tab")?.addEventListener("click", () => {
      this.createNewTab();
    });
  }
}


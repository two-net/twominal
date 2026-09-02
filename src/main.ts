import "./styles/main.css";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { SplashScreen } from "./splash/SplashScreen";
import { ThemeManager } from "./theme/ThemeManager";
import { FontManager } from "./fonts/FontManager";
import { VimModeEngine } from "./vim/VimModeEngine";
import { CompletionMenu } from "./fish/CompletionMenu";
import { Autosuggestions } from "./fish/Autosuggestions";
import { SyntaxHighlighter } from "./fish/SyntaxHighlighter";
import { MatrixRain } from "./matrix/MatrixRain";
import { SettingsModal } from "./ui/SettingsModal";
import { StatusBar } from "./ui/StatusBar";
import { Shortcuts } from "./ui/Shortcuts";
import { TabManager } from "./tabs/TabManager";
import { CommandExecutor, TabSessionData } from "./terminal/CommandExecutor";
import { ansiToHtml, escapeHtml } from "./utils/ansi";

window.addEventListener("DOMContentLoaded", async () => {
  // 1. Splash Screen entrance sequence
  const splash = new SplashScreen();

  // 2. Core Singletons & Managers
  const themeManager = ThemeManager.getInstance();
  const fontManager = FontManager.getInstance();
  const vimEngine = new VimModeEngine();
  const autosuggestions = new Autosuggestions();
  const completionMenu = new CompletionMenu();

  // 4. Matrix Digital Rain Canvas Easter Egg
  const matrixCanvas = document.getElementById("matrix-canvas") as HTMLCanvasElement;
  const matrixRain = new MatrixRain(matrixCanvas);

  // 5. Multi-tab Manager
  const tabsListEl = document.getElementById("tabs-list") as HTMLElement;
  const tabManager = new TabManager(tabsListEl, vimEngine);

  // 6. Modals
  const settingsModal = new SettingsModal(tabManager);

  // 7. Command Execution Engine (NO MOCK - REAL BACKEND)
  const commandExecutor = new CommandExecutor(
    themeManager,
    fontManager,
    vimEngine,
    matrixRain,
    tabManager
  );
  commandExecutor.onTabUpdate(() => {
    tabManager.refreshTabTitles();
  });

  // 8. Status Bar
  new StatusBar(vimEngine, fontManager, themeManager, () => {
    settingsModal.toggle();
  });

  // 9. Global Shortcuts
  Shortcuts.init(tabManager, settingsModal, vimEngine);

  // 10. Setup Header Controls & Dropdown
  setupHeaderControls(settingsModal, vimEngine, themeManager, fontManager, tabManager);

  // 11. Setup Interactive Fish Shell Prompt Layer
  setupFishPromptLayer(tabManager, vimEngine, completionMenu, autosuggestions, commandExecutor);

  // 12. Setup Mobile Helper Keybar
  setupMobileKeybar(tabManager, vimEngine, commandExecutor, autosuggestions);

  // 13. Check if this window was opened from a detached tab or has pending initial tab
  let initialTabPayload: any = null;
  let isDetachedWindow = false;

  try {
    const currentWin = getCurrentWebviewWindow();
    if (currentWin && currentWin.label && currentWin.label !== "main") {
      const stored = localStorage.getItem(`twominal_init_tab_${currentWin.label}`);
      if (stored) {
        initialTabPayload = JSON.parse(stored);
        isDetachedWindow = true;
        localStorage.removeItem(`twominal_init_tab_${currentWin.label}`);
      }
    }
  } catch {
    // web fallback
  }

  if (!initialTabPayload) {
    try {
      const latest = localStorage.getItem("twominal_latest_detached_tab");
      if (latest) {
        const parsed = JSON.parse(latest);
        if (Date.now() - (parsed.timestamp || 0) < 10000) {
          initialTabPayload = parsed;
          isDetachedWindow = true;
          localStorage.removeItem("twominal_latest_detached_tab");
        }
      }
    } catch {}
  }

  if (isDetachedWindow && initialTabPayload) {
    // Fast boot for detached window: immediately hide splash screen
    await splash.hide(50);
    const restoredTab = await tabManager.createTabFromSession(initialTabPayload);
    renderTerminalOutput(tabManager);
    const cliInput = document.getElementById("cli-input") as HTMLInputElement;
    if (cliInput && restoredTab.currentInput) {
      cliInput.value = restoredTab.currentInput;
    }
    focusInput();
  } else {
    // Run Splash progressive status sequence
    await splash.animateSequence();
    // Spawn initial shell tab
    await tabManager.createNewTab();
    renderTerminalOutput(tabManager);
    // Smoothly hide Splash Screen
    await splash.hide(300);
    focusInput();
  }
});


function setupHeaderControls(
  settingsModal: SettingsModal,
  vimEngine: VimModeEngine,
  themeManager: ThemeManager,
  fontManager: FontManager,
  tabManager: TabManager
): void {
  // Vim Mode Toggle Button
  document.getElementById("btn-toggle-vim-mode")?.addEventListener("click", () => {
    vimEngine.toggle();
    const activeTab = tabManager.getActiveTab();
    if (activeTab) {
      activeTab.vimMode = vimEngine.getIsEnabled();
      activeTab.vimState = vimEngine.getMode();
    }
    updateVimUI(vimEngine);
  });

  // Theme Dropdown Menu
  const btnThemeMenu = document.getElementById("btn-theme-menu");
  const themeDropdown = document.getElementById("theme-dropdown");

  btnThemeMenu?.addEventListener("click", (e) => {
    e.stopPropagation();
    themeDropdown?.classList.toggle("hidden");
  });

  window.addEventListener("click", () => {
    themeDropdown?.classList.add("hidden");
  });

  // Theme Dropdown Options
  document.getElementById("theme-opt-dark")?.addEventListener("click", () => {
    themeManager.setMode("dark");
  });

  document.getElementById("theme-opt-light")?.addEventListener("click", () => {
    themeManager.setMode("light");
  });

  document.getElementById("theme-opt-auto")?.addEventListener("click", () => {
    themeManager.setMode("auto");
  });

  document.getElementById("theme-opt-ligatures")?.addEventListener("click", () => {
    fontManager.toggleLigatures();
  });

  // Settings & Help Button
  document.getElementById("btn-open-settings")?.addEventListener("click", () => {
    settingsModal.toggle();
  });

  vimEngine.onModeChange((mode, isEnabled) => {
    const activeTab = tabManager.getActiveTab();
    if (activeTab) {
      activeTab.vimMode = isEnabled;
      activeTab.vimState = mode;
    }
    updateVimUI(vimEngine);
  });

  // Re-render UI on theme changes
  themeManager.onThemeChange(() => {
    tabManager.renderTabs();
    renderTerminalOutput(tabManager);
    const isDark = document.documentElement.classList.contains("dark");
    for (const tab of tabManager.getTabs()) {
      tab.terminalSession?.updateTheme(isDark);
    }
  });

  fontManager.onFontChange((settings) => {
    for (const tab of tabManager.getTabs()) {
      tab.terminalSession?.updateFont(settings);
    }
  });

  window.addEventListener("resize", () => {
    const activeTab = tabManager.getActiveTab();
    if (activeTab?.isPtyRunning) {
      activeTab.terminalSession?.fit();
    }
  });

  // Initial UI sync
  updateVimUI(vimEngine);
}

function updateVimUI(vimEngine: VimModeEngine): void {
  const isEnabled = vimEngine.getIsEnabled();
  const mode = vimEngine.getMode();
  const badge = document.getElementById("vim-mode-badge");
  const btnVim = document.getElementById("btn-toggle-vim-mode");

  if (badge) {
    badge.textContent = isEnabled ? `VIM: ${mode}` : "VIM: OFF";
  }

  if (btnVim) {
    if (isEnabled) {
      if (mode === "NORMAL") {
        btnVim.className = "flex items-center gap-1.5 px-2 py-0.5 rounded-md dark:bg-amber-500/20 bg-amber-100 dark:text-amber-300 text-amber-900 dark:border-amber-500/40 border-amber-400 border transition-colors shadow-xs cursor-pointer";
      } else if (mode === "VISUAL") {
        btnVim.className = "flex items-center gap-1.5 px-2 py-0.5 rounded-md dark:bg-purple-500/20 bg-purple-100 dark:text-purple-300 text-purple-900 dark:border-purple-500/40 border-purple-400 border transition-colors shadow-xs cursor-pointer";
      } else {
        btnVim.className = "flex items-center gap-1.5 px-2 py-0.5 rounded-md dark:bg-cyan-500/20 bg-cyan-100 dark:text-cyan-300 text-cyan-900 dark:border-cyan-500/40 border-cyan-400 border transition-colors shadow-xs cursor-pointer";
      }
    } else {
      btnVim.className = "flex items-center gap-1.5 px-2 py-0.5 rounded-md dark:bg-slate-800 bg-slate-200 dark:text-slate-400 text-slate-600 dark:border-slate-700 border-slate-300 border transition-colors shadow-xs cursor-pointer opacity-75";
    }
  }
}

function setupFishPromptLayer(
  tabManager: TabManager,
  vimEngine: VimModeEngine,
  completionMenu: CompletionMenu,
  autosuggestions: Autosuggestions,
  commandExecutor: CommandExecutor
): void {
  const cliInput = document.getElementById("cli-input") as HTMLInputElement;
  const inputRendered = document.getElementById("input-rendered");
  const ghostEl = document.getElementById("autosuggest-ghost");

  commandExecutor.onRender(() => {
    renderTerminalOutput(tabManager);
    updatePromptBadges(tabManager.getActiveTab());
  });

  const updateSyntaxAndAutosuggest = async () => {
    const text = cliInput.value;
    const tab = tabManager.getActiveTab();
    if (tab) tab.currentInput = text;

    // 1. Highlight tokens
    if (inputRendered) {
      const tokens = SyntaxHighlighter.tokenize(text);
      inputRendered.innerHTML = SyntaxHighlighter.toHtml(tokens);
    }

    // 2. Autosuggestion ghost text
    if (ghostEl) {
      if (text.trim().length > 0) {
        // Fast sync check (history, known commands, cached paths)
        const suggestion = autosuggestions.getSuggestion(text, tab?.cwd);
        if (suggestion && suggestion.toLowerCase().startsWith(text.toLowerCase())) {
          ghostEl.textContent = suggestion.slice(text.length);
        } else {
          ghostEl.textContent = "";
          // Async background fetch if not yet found
          const asyncSug = await autosuggestions.getSuggestionAsync(text, tab?.cwd);
          if (cliInput.value === text) {
            if (asyncSug && asyncSug.toLowerCase().startsWith(text.toLowerCase())) {
              ghostEl.textContent = asyncSug.slice(text.length);
            } else {
              ghostEl.textContent = "";
            }
          }
        }
      } else {
        ghostEl.textContent = "";
      }
    }

    // 3. Live Slash Builtin Command Palette
    if (text.startsWith("/") && !text.includes(" ")) {
      const matches = completionMenu.filterBuiltinCommands(text);
      if (matches.length > 0) {
        completionMenu.showCustomItems(matches, (item) => {
          cliInput.value = item.value + " ";
          updateSyntaxAndAutosuggest();
          focusInput();
        });
      } else {
        completionMenu.hide();
      }
    } else if (completionMenu.getIsOpen() && !text.startsWith("/")) {
      completionMenu.hide();
    }
  };

  SyntaxHighlighter.onUpdate(() => {
    updateSyntaxAndAutosuggest();
  });

  autosuggestions.onUpdate(() => {
    updateSyntaxAndAutosuggest();
  });

  cliInput.addEventListener("input", async () => {
    const tab = tabManager.getActiveTab();
    if (tab?.isPtyRunning) {
      if (cliInput.value) {
        await invoke("pty_write", { id: tab.id, data: cliInput.value });
        cliInput.value = "";
      }
      return;
    }
    if (vimEngine.getIsEnabled() && vimEngine.getMode() === "NORMAL") return;
    updateSyntaxAndAutosuggest();
  });

  cliInput.addEventListener("keydown", async (e) => {
    const tab = tabManager.getActiveTab();
    if (!tab) return;

    // 0. Active PTY process routing (agy, vim, top, python, streaming commands)
    if (tab.isPtyRunning) {
      if (e.ctrlKey || e.metaKey) {
        if (e.key === "c" || e.key === "C") {
          e.preventDefault();
          await commandExecutor.cancelCommand(tab);
          cliInput.value = "";
          updateSyntaxAndAutosuggest();
          return;
        }
        if (e.key === "d" || e.key === "D") {
          e.preventDefault();
          await invoke("pty_write", { id: tab.id, data: "\x04" });
          return;
        }
        if (e.key === "z" || e.key === "Z") {
          e.preventDefault();
          await invoke("pty_write", { id: tab.id, data: "\x1a" });
          return;
        }
      }

      if (e.key === "Enter") {
        e.preventDefault();
        await invoke("pty_write", { id: tab.id, data: "\r" });
        cliInput.value = "";
        return;
      }
      if (e.key === "Backspace") {
        e.preventDefault();
        await invoke("pty_write", { id: tab.id, data: "\x7f" });
        return;
      }
      if (e.key === "Tab") {
        e.preventDefault();
        await invoke("pty_write", { id: tab.id, data: "\t" });
        return;
      }
      if (e.key === "ArrowUp") {
        e.preventDefault();
        await invoke("pty_write", { id: tab.id, data: "\x1b[A" });
        return;
      }
      if (e.key === "ArrowDown") {
        e.preventDefault();
        await invoke("pty_write", { id: tab.id, data: "\x1b[B" });
        return;
      }
      if (e.key === "ArrowRight") {
        e.preventDefault();
        await invoke("pty_write", { id: tab.id, data: "\x1b[C" });
        return;
      }
      if (e.key === "ArrowLeft") {
        e.preventDefault();
        await invoke("pty_write", { id: tab.id, data: "\x1b[D" });
        return;
      }
      if (e.key === "Escape") {
        e.preventDefault();
        await invoke("pty_write", { id: tab.id, data: "\x1b" });
        return;
      }
      if (e.key.length === 1 && !e.ctrlKey && !e.metaKey && !e.altKey) {
        e.preventDefault();
        await invoke("pty_write", { id: tab.id, data: e.key });
        return;
      }
      return;
    }

    // 0.5. Active non-PTY process routing (shell_exec commands like streaming builds/scripts)
    if (!tab.isPtyRunning && tab.activeProcess && tab.activeProcess !== "fish") {
      if ((e.ctrlKey || e.metaKey) && (e.key === "c" || e.key === "C")) {
        e.preventDefault();
        await commandExecutor.cancelCommand(tab);
        cliInput.value = "";
        updateSyntaxAndAutosuggest();
        return;
      }
      if (e.key === "Enter") {
        e.preventDefault();
        if (cliInput.value) {
          await invoke("shell_write", { tabId: tab.id, data: cliInput.value + "\n" });
          cliInput.value = "";
          updateSyntaxAndAutosuggest();
        }
        return;
      }
    }

    // 1. Vim Mode Normal handling
    if (vimEngine.getIsEnabled() && vimEngine.getMode() === "NORMAL") {
      if (e.key === "Enter") {
        e.preventDefault();
        await runCommand(tab, cliInput.value, commandExecutor, tabManager, vimEngine, autosuggestions);
        return;
      }
      const res = vimEngine.handleKey(
        e.key,
        e.ctrlKey,
        e.altKey,
        cliInput.value,
        tab.vimCursorPos
      );
      if (res.handled) {
        e.preventDefault();
        cliInput.value = res.newBuffer;
        tab.vimCursorPos = res.newCursor;
        updateSyntaxAndAutosuggest();
        if (res.action === "submit") {
          await runCommand(tab, cliInput.value, commandExecutor, tabManager, vimEngine, autosuggestions);
        }
        return;
      }
    }

    // 2. Escape key handling
    if (e.key === "Escape") {
      if (completionMenu.getIsOpen()) {
        completionMenu.hide();
        return;
      }
      if (vimEngine.getIsEnabled()) {
        vimEngine.setMode("NORMAL");
        tab.vimCursorPos = Math.max(0, cliInput.value.length - 1);
        updateVimUI(vimEngine);
        updateSyntaxAndAutosuggest();
        e.preventDefault();
        return;
      }
    }

    // 3. Tab completion popup menu navigation
    if (completionMenu.getIsOpen()) {
      if (e.key === "ArrowDown" || (e.key === "Tab" && !e.shiftKey)) {
        e.preventDefault();
        completionMenu.selectNext();
        return;
      }
      if (e.key === "ArrowUp" || (e.key === "Tab" && e.shiftKey)) {
        e.preventDefault();
        completionMenu.selectPrevious();
        return;
      }
      if (e.key === "Enter") {
        e.preventDefault();
        completionMenu.confirmSelection();
        return;
      }
    }

    // 4. Tab Key: trigger completions or accept ghost
    if (e.key === "Tab" && !e.shiftKey && !e.ctrlKey && !e.altKey) {
      e.preventDefault();
      const ghost = ghostEl?.textContent;
      if (ghost) {
        const fullSuggestion = autosuggestions.getLastFullSuggestion();
        cliInput.value = autosuggestions.acceptFull(cliInput.value, fullSuggestion);
        updateSyntaxAndAutosuggest();
        return;
      }

      await completionMenu.trigger(
        tab.cwd || ".",
        cliInput.value,
        (item) => {
          cliInput.value = autosuggestions.applyCompletion(cliInput.value, item);
          updateSyntaxAndAutosuggest();
          focusInput();
        }
      );
      return;
    }

    // 5. Right Arrow Key: Accept autosuggestion
    if (e.key === "ArrowRight") {
      const ghost = ghostEl?.textContent;
      if (ghost && cliInput.selectionStart === cliInput.value.length) {
        const fullSuggestion = autosuggestions.getLastFullSuggestion();
        cliInput.value = autosuggestions.acceptFull(cliInput.value, fullSuggestion);
        updateSyntaxAndAutosuggest();
        e.preventDefault();
        return;
      }
    }

    // Ctrl + F / Cmd + ArrowRight: Accept autosuggestion
    if ((e.ctrlKey || e.metaKey) && (e.key === "f" || e.key === "F" || e.key === "ArrowRight")) {
      const ghost = ghostEl?.textContent;
      if (ghost) {
        const fullSuggestion = autosuggestions.getLastFullSuggestion();
        cliInput.value = autosuggestions.acceptFull(cliInput.value, fullSuggestion);
        updateSyntaxAndAutosuggest();
        e.preventDefault();
        return;
      }
    }

    // Alt + Right Arrow Key: Accept next word of autosuggestion
    if (e.altKey && e.key === "ArrowRight") {
      const fullSuggestion = autosuggestions.getLastFullSuggestion() || autosuggestions.getSuggestion(cliInput.value, tab.cwd);
      if (fullSuggestion) {
        const nextBuffer = autosuggestions.acceptNextWord(cliInput.value, fullSuggestion);
        if (nextBuffer !== cliInput.value) {
          cliInput.value = nextBuffer;
          updateSyntaxAndAutosuggest();
          e.preventDefault();
          return;
        }
      }
    }

    // 6. Enter Key: Run command
    if (e.key === "Enter") {
      e.preventDefault();
      await runCommand(tab, cliInput.value, commandExecutor, tabManager, vimEngine, autosuggestions);
      return;
    }

    // 7. Up / Down Arrow: Command History
    if (e.key === "ArrowUp") {
      e.preventDefault();
      if (tab.history.length > 0) {
        if (tab.historyIndex === -1 || tab.historyIndex > tab.history.length) {
          tab.historyIndex = tab.history.length;
        }
        if (tab.historyIndex > 0) {
          tab.historyIndex--;
          cliInput.value = tab.history[tab.historyIndex] || "";
          updateSyntaxAndAutosuggest();
        }
      }
      return;
    }

    if (e.key === "ArrowDown") {
      e.preventDefault();
      if (tab.historyIndex < tab.history.length - 1) {
        tab.historyIndex++;
        cliInput.value = tab.history[tab.historyIndex] || "";
        updateSyntaxAndAutosuggest();
      } else {
        tab.historyIndex = tab.history.length;
        cliInput.value = "";
        tab.currentInput = "";
        updateSyntaxAndAutosuggest();
      }
      return;
    }

    // 8. Shortcuts: Ctrl+C / Ctrl+L / Ctrl+T / Ctrl+W
    if (e.ctrlKey || e.metaKey) {
      if (e.key === "c" || e.key === "C") {
        await commandExecutor.cancelCommand(tab);
        tab.outputHistory.push({
          type: "prompt",
          cwd: tab.displayCwd || tab.cwd,
          git: tab.gitBranch,
          cmd: cliInput.value + "^C",
        });
        renderTerminalOutput(tabManager);
        cliInput.value = "";
        tab.currentInput = "";
        updateSyntaxAndAutosuggest();
        e.preventDefault();
      } else if (e.key === "l" || e.key === "L") {
        tab.outputHistory = [];
        renderTerminalOutput(tabManager);
        cliInput.value = "";
        tab.currentInput = "";
        updateSyntaxAndAutosuggest();
        e.preventDefault();
      }
    }
  });

  // Keep CLI input focused when clicking anywhere in window or when window regains focus
  window.addEventListener("click", (e: MouseEvent) => {
    const isModalOpen =
      document.getElementById("modal-settings")?.classList.contains("hidden") === false;
    if (isModalOpen) return;

    const selection = window.getSelection();
    if (selection && selection.toString().length > 0) return;

    const target = e.target as HTMLElement | null;
    if (
      target &&
      target !== cliInput &&
      (target.tagName === "INPUT" || target.tagName === "TEXTAREA" || target.isContentEditable)
    ) {
      return;
    }

    const activeTab = tabManager.getActiveTab();
    if (activeTab?.isPtyRunning) {
      activeTab.terminalSession?.focus();
    } else {
      focusInput();
    }
  });

  window.addEventListener("focus", () => {
    const isModalOpen =
      document.getElementById("modal-settings")?.classList.contains("hidden") === false;
    if (!isModalOpen) {
      const activeTab = tabManager.getActiveTab();
      if (activeTab?.isPtyRunning) {
        activeTab.terminalSession?.focus();
      } else {
        focusInput();
      }
    }
  });

  // Redirect keypresses to input if focus was lost and no modal is active
  window.addEventListener("keydown", (e: KeyboardEvent) => {
    const isModalOpen =
      document.getElementById("modal-settings")?.classList.contains("hidden") === false;
    if (isModalOpen) return;

    const activeTab = tabManager.getActiveTab();
    if (activeTab?.isPtyRunning) {
      activeTab.terminalSession?.focus();
      return;
    }

    const activeEl = document.activeElement;
    if (
      activeEl &&
      activeEl !== cliInput &&
      (activeEl.tagName === "INPUT" || activeEl.tagName === "TEXTAREA" || (activeEl as HTMLElement).isContentEditable)
    ) {
      return;
    }

    if (document.activeElement !== cliInput && !e.ctrlKey && !e.metaKey && !e.altKey) {
      cliInput.focus();
    }
  });

  // Listen to tab changes
  tabManager.onTabChange((activeTab) => {
    if (!activeTab) return;

    const ptyLayer = document.getElementById("terminal-pty-layer");
    const promptRow = document.getElementById("prompt-row");

    if (activeTab.isPtyRunning && activeTab.terminalSession) {
      if (ptyLayer) {
        ptyLayer.classList.remove("hidden");
        activeTab.terminalSession.mount(ptyLayer);
      }
      if (promptRow) {
        promptRow.classList.add("hidden");
      }
    } else {
      if (ptyLayer) {
        ptyLayer.classList.add("hidden");
      }
      if (promptRow) {
        promptRow.classList.remove("hidden");
      }
    }

    updatePromptBadges(activeTab);
    cliInput.value = activeTab.currentInput || "";
    vimEngine.setEnabled(activeTab.vimMode, false);
    vimEngine.setMode(activeTab.vimState);
    updateVimUI(vimEngine);
    renderTerminalOutput(tabManager);
    updateSyntaxAndAutosuggest();
    if (!activeTab.isPtyRunning) {
      focusInput();
    }
  });
}

function updatePromptBadges(tab: TabSessionData | null): void {
  if (!tab) return;
  const promptCwd = document.getElementById("prompt-cwd");
  const promptGitBadge = document.getElementById("prompt-git-badge");
  const promptGit = document.getElementById("prompt-git");

  const isRunning = Boolean(
    tab.isPtyRunning ||
    (tab.activeProcess && tab.activeProcess !== "fish") ||
    tab.activeStreamText !== undefined
  );

  if (promptCwd) {
    if (isRunning) {
      promptCwd.innerHTML = `<span class="dark:text-amber-400 text-amber-600 font-bold animate-pulse">● running ${escapeHtml(tab.activeProcess || "process")} (Ctrl+C to stop)</span>`;
    } else {
      promptCwd.textContent = tab.displayCwd || tab.cwd || "~";
    }
  }

  if (promptGitBadge && promptGit) {
    if (!isRunning && tab.gitBranch && tab.gitBranch.trim().length > 0) {
      promptGit.textContent = tab.gitBranch.trim();
      promptGitBadge.classList.remove("hidden");
    } else {
      promptGit.textContent = "";
      promptGitBadge.classList.add("hidden");
    }
  }
}

async function runCommand(
  tab: TabSessionData,
  cmd: string,
  commandExecutor: CommandExecutor,
  tabManager: TabManager,
  vimEngine?: VimModeEngine,
  autosuggestions?: Autosuggestions
): Promise<void> {
  const cliInput = document.getElementById("cli-input") as HTMLInputElement;
  const inputRendered = document.getElementById("input-rendered");
  const ghostEl = document.getElementById("autosuggest-ghost");

  // 1. Immediately reset CLI input, tab currentInput, and rendered visual layers
  if (cliInput) {
    cliInput.value = "";
  }
  tab.currentInput = "";
  if (inputRendered) {
    inputRendered.innerHTML = "";
  }
  if (ghostEl) {
    ghostEl.textContent = "";
  }

  if (tab.vimMode) {
    tab.vimState = "INSERT";
    tab.vimCursorPos = 0;
    if (vimEngine && vimEngine.getIsEnabled()) {
      vimEngine.setMode("INSERT");
      updateVimUI(vimEngine);
    }
  }

  // 2. Execute command
  await commandExecutor.execute(tab, cmd);
  autosuggestions?.clearCache();
  renderTerminalOutput(tabManager);
  updatePromptBadges(tab);

  // 3. Ensure input line remains cleared and properly focused
  if (cliInput) {
    cliInput.value = "";
  }
  tab.currentInput = "";
  if (inputRendered) {
    inputRendered.innerHTML = "";
  }
  if (ghostEl) {
    ghostEl.textContent = "";
  }

  cliInput?.dispatchEvent(new Event("input", { bubbles: true }));
  if (!tab.isPtyRunning) {
    focusInput();
  }
}

function renderTerminalOutput(tabManager: TabManager): void {
  const outputContainer = document.getElementById("terminal-output");
  const tab = tabManager.getActiveTab();
  if (!outputContainer || !tab) return;

  outputContainer.innerHTML = "";

  tab.outputHistory.forEach((item) => {
    const row = document.createElement("div");
    if (item.type === "prompt") {
      row.className = "flex items-center gap-2 text-xs sm:text-sm font-mono my-0.5";
      const highlighted = SyntaxHighlighter.toHtml(SyntaxHighlighter.tokenize(item.cmd || ""));
      const gitBadgeHtml = item.git && item.git.trim().length > 0
        ? `<span class="dark:text-emerald-400 text-emerald-700 font-medium">(${escapeHtml(item.git)})</span>`
        : "";
      row.innerHTML = `
        <span class="dark:text-indigo-400 text-indigo-700 font-semibold">${escapeHtml(item.cwd || "~")}</span>
        ${gitBadgeHtml}
        <span class="dark:text-cyan-400 text-blue-600 font-bold">➜</span>
        <span class="dark:text-slate-100 text-slate-800 font-mono whitespace-pre">${highlighted}</span>
      `;
    } else if (item.type === "response" && item.html) {
      row.innerHTML = item.html;
    }
    outputContainer.appendChild(row);
  });

  if (tab.activeStreamText) {
    const streamRow = document.createElement("div");
    streamRow.className = "font-mono text-xs sm:text-sm whitespace-pre-wrap leading-relaxed select-text py-0.5 opacity-90";
    streamRow.innerHTML = ansiToHtml(tab.activeStreamText);
    outputContainer.appendChild(streamRow);
  }

  const viewport = document.getElementById("terminal-viewport");
  if (viewport) {
    viewport.scrollTop = viewport.scrollHeight;
  }
}

function setupMobileKeybar(
  tabManager: TabManager,
  vimEngine: VimModeEngine,
  commandExecutor: CommandExecutor,
  autosuggestions?: Autosuggestions
): void {
  document.getElementById("vkey-tab")?.addEventListener("click", () => {
    const cliInput = document.getElementById("cli-input") as HTMLInputElement;
    cliInput?.dispatchEvent(new KeyboardEvent("keydown", { key: "Tab", bubbles: true }));
  });

  document.getElementById("vkey-auto")?.addEventListener("click", () => {
    const cliInput = document.getElementById("cli-input") as HTMLInputElement;
    const ghostEl = document.getElementById("autosuggest-ghost");
    if (cliInput && ghostEl?.textContent) {
      const fullSuggestion = autosuggestions?.getLastFullSuggestion();
      if (autosuggestions && fullSuggestion) {
        cliInput.value = autosuggestions.acceptFull(cliInput.value, fullSuggestion);
      } else {
        cliInput.value += ghostEl.textContent;
      }
      cliInput.dispatchEvent(new Event("input", { bubbles: true }));
    }
  });

  document.getElementById("vkey-esc")?.addEventListener("click", () => {
    if (vimEngine.getIsEnabled()) {
      vimEngine.setMode("NORMAL");
      updateVimUI(vimEngine);
    }
  });

  document.getElementById("vkey-clear")?.addEventListener("click", () => {
    const tab = tabManager.getActiveTab();
    if (tab) {
      tab.outputHistory = [];
      tab.currentInput = "";
      const cliInput = document.getElementById("cli-input") as HTMLInputElement;
      if (cliInput) cliInput.value = "";
      const inputRendered = document.getElementById("input-rendered");
      if (inputRendered) inputRendered.innerHTML = "";
      const ghostEl = document.getElementById("autosuggest-ghost");
      if (ghostEl) ghostEl.textContent = "";
      renderTerminalOutput(tabManager);
      focusInput();
    }
  });

  document.getElementById("vkey-help")?.addEventListener("click", () => {
    const tab = tabManager.getActiveTab();
    if (tab) {
      runCommand(tab, "help", commandExecutor, tabManager, vimEngine, autosuggestions);
    }
  });
}

function focusInput(): void {
  const cliInput = document.getElementById("cli-input");
  cliInput?.focus();
}

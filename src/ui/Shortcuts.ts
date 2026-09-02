import { TabManager } from "../tabs/TabManager";
import { SettingsModal } from "./SettingsModal";
import { VimModeEngine } from "../vim/VimModeEngine";
import { ThemeManager, ThemeMode } from "../theme/ThemeManager";
import { FontManager } from "../fonts/FontManager";

export class Shortcuts {
  public static init(
    tabManager: TabManager,
    settingsModal: SettingsModal,
    vimEngine: VimModeEngine
  ): () => void {
    const handler = (e: KeyboardEvent) => {
      const isCmdOrCtrl = e.metaKey || e.ctrlKey;

      // Cmd/Ctrl + T: New Tab
      if (isCmdOrCtrl && e.key.toLowerCase() === "t" && !e.shiftKey && !e.altKey) {
        e.preventDefault();
        tabManager.createNewTab();
        return;
      }

      // Cmd/Ctrl + N: New App Window
      if (isCmdOrCtrl && e.key.toLowerCase() === "n" && !e.shiftKey && !e.altKey) {
        e.preventDefault();
        const activeTab = tabManager.getActiveTab();
        tabManager.createNewWindow(activeTab?.cwd);
        return;
      }

      // Cmd/Ctrl + Shift + D: Move/Detach Active Tab to New Window
      if (isCmdOrCtrl && e.shiftKey && e.key.toLowerCase() === "d" && !e.altKey) {
        e.preventDefault();
        const activeTab = tabManager.getActiveTab();
        if (activeTab) {
          tabManager.detachTabToNewWindow(activeTab.id);
        }
        return;
      }

      // Cmd/Ctrl + W: Close Active Tab
      if (isCmdOrCtrl && e.key.toLowerCase() === "w" && !e.shiftKey && !e.altKey) {
        e.preventDefault();
        tabManager.closeActiveTab();
        return;
      }

      // Cmd/Ctrl + ,: Open Settings & Guide
      if (isCmdOrCtrl && e.key === ",") {
        e.preventDefault();
        settingsModal.toggle();
        return;
      }

      // Ctrl + Shift + V or Cmd + Shift + V: Toggle Vim Mode
      if (isCmdOrCtrl && e.shiftKey && e.key.toLowerCase() === "v") {
        e.preventDefault();
        vimEngine.toggle();
        return;
      }

      // Cmd/Ctrl + Shift + T: Cycle Theme Mode
      if (isCmdOrCtrl && e.shiftKey && e.key.toLowerCase() === "t") {
        e.preventDefault();
        const themeManager = ThemeManager.getInstance();
        const modes: ThemeMode[] = ["dark", "light", "auto"];
        const nextIdx = (modes.indexOf(themeManager.getMode()) + 1) % modes.length;
        themeManager.setMode(modes[nextIdx]);
        return;
      }

      // Cmd/Ctrl + = / + : Increase font size
      if (isCmdOrCtrl && (e.key === "=" || e.key === "+")) {
        e.preventDefault();
        FontManager.getInstance().increaseFontSize(1);
        return;
      }

      // Cmd/Ctrl + - / _ : Decrease font size
      if (isCmdOrCtrl && (e.key === "-" || e.key === "_")) {
        e.preventDefault();
        FontManager.getInstance().decreaseFontSize(1);
        return;
      }

      // Cmd/Ctrl + 0 : Reset font size to default
      if (isCmdOrCtrl && e.key === "0") {
        e.preventDefault();
        FontManager.getInstance().resetFontSize();
        return;
      }

      // Cmd/Ctrl + 1..9: Switch to Tab 1..9
      if (isCmdOrCtrl && !e.shiftKey && !e.altKey && /^[1-9]$/.test(e.key)) {
        e.preventDefault();
        const tabIndex = parseInt(e.key, 10) - 1;
        tabManager.switchToTabIndex(tabIndex);
        return;
      }

      // Ctrl + Tab: Next Tab, Ctrl + Shift + Tab: Prev Tab
      if (e.ctrlKey && e.key === "Tab") {
        e.preventDefault();
        if (e.shiftKey) {
          tabManager.switchToPrevTab();
        } else {
          tabManager.switchToNextTab();
        }
        return;
      }
    };

    window.addEventListener("keydown", handler, { capture: true });
    return () => window.removeEventListener("keydown", handler, { capture: true });
  }
}

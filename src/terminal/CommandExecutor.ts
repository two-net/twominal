import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { ansiToHtml, escapeHtml } from "../utils/ansi";
import { HistoryManager } from "../fish/HistoryManager";
import { ThemeManager, ThemeMode } from "../theme/ThemeManager";
import { FontManager } from "../fonts/FontManager";
import { VimModeEngine } from "../vim/VimModeEngine";
import { MatrixRain } from "../matrix/MatrixRain";
import { TabManager } from "../tabs/TabManager";
import { TerminalSession } from "./TerminalSession";

const INTERACTIVE_COMMANDS = new Set([
  "vi", "vim", "nvim", "nano", "emacs", "pico", "joe", "micro", "helix", "hx",
  "top", "htop", "btop", "atop", "glances", "nvtop", "iotop", "iftop",
  "less", "more", "man", "info",
  "ssh", "telnet", "ftp", "sftp", "mosh",
  "tmux", "screen", "zellij",
  "lazygit", "gitui", "tig",
  "fzf", "ranger", "nnn", "mc", "yazi", "broot",
  "k9s", "lazydocker",
  "agy"
]);

export function isInteractiveCommand(mainCmd: string, args: string[]): boolean {
  const baseCmd = mainCmd.toLowerCase().trim();
  if (INTERACTIVE_COMMANDS.has(baseCmd)) {
    return true;
  }
  if (baseCmd === "sudo" && args.length > 0) {
    return isInteractiveCommand(args[0], args.slice(1));
  }
  // Dotnet CLI and applications: e.g. "dotnet run", "dotnet watch", "dotnet test", "dotnet new", "dotnet tool"
  // ALWAYS run dotnet with native PTY so stdin, Console.ReadLine(), Console.ReadKey(), Spectre.Console,
  // and real-time streaming output are 100% responsive and never blocked!
  if (baseCmd === "dotnet") {
    return true;
  }
  // Local compiled binaries or executable scripts (e.g. "./my-dotnet-cli", "./app", "../bin/tool", "~/bin/cli")
  // User-compiled binaries or scripts are typically interactive or streaming CLI applications
  if (
    baseCmd.startsWith("./") ||
    baseCmd.startsWith("../") ||
    baseCmd.startsWith("/") ||
    baseCmd.startsWith("~/")
  ) {
    return true;
  }
  // Common language package/build runners & interactive CLIs
  if (
    [
      "cargo", "npm", "npx", "pnpm", "yarn", "bun", "deno",
      "python", "python3", "ipython", "node", "ruby", "irb", "rails", "bundle",
      "go", "gradle", "mvn", "sbt", "make", "cmake", "docker", "podman", "docker-compose",
      "kubectl", "minikube", "helm", "gh", "glab", "psql", "mysql", "sqlite3", "redis-cli", "mongosh",
      "ping", "traceroute", "watch", "tail", "mtr"
    ].includes(baseCmd)
  ) {
    return true;
  }
  // Shells without -c or script args (e.g. "bash", "zsh", "fish")
  if (["bash", "zsh", "fish", "sh", "csh", "tcsh", "ksh"].includes(baseCmd)) {
    return !args.includes("-c") && args.length === 0;
  }
  // Interactive git commands
  if (baseCmd === "git") {
    const sub = args[0]?.toLowerCase();
    if (
      [
        "commit", "rebase", "add", "diff", "log", "show", "branch",
        "merge", "checkout", "switch", "push", "pull", "fetch", "clone"
      ].includes(sub)
    ) {
      return true;
    }
    if (args.some((a) => a === "-p" || a === "--patch" || a === "-i" || a === "--interactive")) {
      return true;
    }
  }
  // Interactive / watch / streaming flags
  if (
    args.some(
      (a) =>
        a === "-i" ||
        a === "--interactive" ||
        a === "-w" ||
        a === "--watch" ||
        a === "-f" ||
        a === "--follow" ||
        a === "--listen" ||
        a === "-l"
    )
  ) {
    return true;
  }
  return false;
}

export interface OutputItem {
  type: "prompt" | "response";
  cwd?: string;
  git?: string;
  cmd?: string;
  html?: string;
}

export interface TabSessionData {
  id: string;
  title: string;
  customTitle?: string | null;
  activeProcess?: string;
  cwd: string;
  displayCwd: string;
  gitBranch: string;
  history: string[];
  historyIndex: number;
  outputHistory: OutputItem[];
  currentInput: string;
  vimMode: boolean;
  vimState: "INSERT" | "NORMAL" | "VISUAL";
  vimCursorPos: number;
  isPtyRunning?: boolean;
  activeStreamText?: string;
  terminalSession?: TerminalSession;
}

export interface ShellExecResult {
  stdout: string;
  stderr: string;
  exit_code: number;
  new_cwd: string;
  display_cwd: string;
  git_branch?: string;
}

export interface SystemInfo {
  os: string;
  host: string;
  user: string;
  shell: string;
  home_dir: string;
  cwd: string;
  display_cwd: string;
  kernel: string;
  arch: string;
}

export class CommandExecutor {
  private themeManager: ThemeManager;
  private fontManager: FontManager;
  private vimEngine: VimModeEngine;
  private matrixRain: MatrixRain;
  private historyManager: HistoryManager;
  private tabManager?: TabManager;
  private onTabUpdateCallback?: () => void;
  private onRenderCallback?: () => void;

  constructor(
    themeManager: ThemeManager,
    fontManager: FontManager,
    vimEngine: VimModeEngine,
    matrixRain: MatrixRain,
    tabManager?: TabManager
  ) {
    this.themeManager = themeManager;
    this.fontManager = fontManager;
    this.vimEngine = vimEngine;
    this.matrixRain = matrixRain;
    this.tabManager = tabManager;
    this.historyManager = HistoryManager.getInstance();
  }

  public setTabManager(tabManager: TabManager): void {
    this.tabManager = tabManager;
  }

  public onTabUpdate(cb: () => void): void {
    this.onTabUpdateCallback = cb;
  }

  public onRender(cb: () => void): void {
    this.onRenderCallback = cb;
  }

  public destroy(): void {
    // Cleanup if needed
  }

  public async cancelCommand(tab: TabSessionData): Promise<void> {
    try {
      await invoke("pty_write", { id: tab.id, data: "\x03" });
      await invoke("pty_kill", { id: tab.id });
      await invoke("shell_cancel", { tabId: tab.id });
    } catch {
      // Ignore
    }
    tab.terminalSession?.stop();
    tab.isPtyRunning = false;
    tab.activeStreamText = undefined;
    tab.activeProcess = "fish";

    const ptyLayer = document.getElementById("terminal-pty-layer");
    if (ptyLayer) {
      ptyLayer.classList.add("hidden");
    }

    const promptRow = document.getElementById("prompt-row");
    if (promptRow) {
      promptRow.classList.remove("hidden");
    }

    this.onTabUpdateCallback?.();
    this.onRenderCallback?.();
  }

  public async execute(tab: TabSessionData, rawCommand: string): Promise<void> {
    const cmd = (rawCommand || "").trim();
    tab.currentInput = "";

    // Close completion menu if open
    document.getElementById("completion-menu")?.classList.add("hidden");

    if (!cmd) {
      tab.outputHistory.push({
        type: "prompt",
        cwd: tab.displayCwd || tab.cwd,
        git: tab.gitBranch,
        cmd: "",
      });
      return;
    }

    // Save to tab history and backend persistent history
    if (tab.history[tab.history.length - 1] !== cmd) {
      tab.history.push(cmd);
    }
    tab.historyIndex = tab.history.length;
    await this.historyManager.add(cmd);

    // Add prompt entry to output buffer
    tab.outputHistory.push({
      type: "prompt",
      cwd: tab.displayCwd || tab.cwd,
      git: tab.gitBranch,
      cmd: cmd,
    });
    this.onRenderCallback?.();

    const parts = cmd.split(/\s+/).filter(Boolean);
    let mainCmd = parts[0].toLowerCase();
    const args = parts.slice(1);

    // Normalize slash command for builtins (e.g. "/theme" -> "theme", "/" -> "help")
    if (mainCmd === "/") {
      mainCmd = "help";
    } else if (mainCmd.startsWith("/")) {
      mainCmd = mainCmd.slice(1);
    }

    let responseHTML = "";

    // 1. Built-in Commands
    switch (mainCmd) {
      case "help":
        responseHTML = `
          <div class="text-xs space-y-1.5 my-1 font-mono">
            <div class="font-bold dark:text-cyan-300 text-blue-600 flex items-center justify-between">
              <span>Twominal Builtin &amp; Slash Commands:</span>
              <span class="text-[11px] font-normal dark:text-slate-400 text-slate-500">Type <code class="dark:text-cyan-300 text-blue-600 font-bold">/</code> for command palette</span>
            </div>
            <div class="grid grid-cols-1 sm:grid-cols-2 gap-2 dark:text-slate-300 text-slate-700">
              <div><span class="dark:text-emerald-400 text-emerald-700 font-bold">/help</span> or <span class="dark:text-emerald-400 text-emerald-700 font-bold">help</span> : List all built-in commands &amp; shortcuts</div>
              <div><span class="dark:text-emerald-400 text-emerald-700 font-bold">/theme [dark|light|auto]</span> : Configure appearance mode</div>
              <div><span class="dark:text-emerald-400 text-emerald-700 font-bold">/font [family|size|reset]</span> : Configure typography &amp; ligatures</div>
              <div><span class="dark:text-emerald-400 text-emerald-700 font-bold">/vim</span> : Toggle Vim modal navigation mode</div>
              <div><span class="dark:text-emerald-400 text-emerald-700 font-bold">/matrix</span> : Toggle background matrix digital rain</div>
              <div><span class="dark:text-emerald-400 text-emerald-700 font-bold">/settings</span> : Open settings &amp; typography modal</div>
              <div><span class="dark:text-emerald-400 text-emerald-700 font-bold">/neofetch</span> : Twominal system information banner</div>
              <div><span class="dark:text-emerald-400 text-emerald-700 font-bold">/clear</span> : Clear terminal scrollback buffer</div>
              <div><span class="dark:text-emerald-400 text-emerald-700 font-bold">/ligatures</span> : Test or toggle font ligatures</div>
              <div><span class="dark:text-emerald-400 text-emerald-700 font-bold">/stack</span> : Architecture &amp; engine info</div>
              <div><span class="dark:text-emerald-400 text-emerald-700 font-bold">/history</span> : View persistent command history</div>
              <div><span class="dark:text-emerald-400 text-emerald-700 font-bold">/date</span> : Display current system timestamp</div>
              <div><span class="dark:text-emerald-400 text-emerald-700 font-bold">/tabs [new|close|next|prev]</span> : Multi-tab window management</div>
              <div><span class="dark:text-emerald-400 text-emerald-700 font-bold">/exit</span> : Close active shell tab session</div>
            </div>
            <div class="dark:text-slate-400 text-slate-500 text-[11px] mt-1">Tip: Built-in commands can be run with or without <code class="dark:text-cyan-300 text-blue-600 font-bold">/</code> prefix. Press <kbd class="px-1 py-0.5 rounded dark:bg-slate-800 dark:border-slate-700 bg-slate-200 border-slate-300 border">Ctrl+C</kbd> to cancel any running process.</div>
          </div>
        `;
        break;


      case "clear":
        tab.outputHistory = [];
        return;

      case "matrix":
        this.matrixRain.toggle();
        responseHTML = `<div class="dark:text-emerald-400 text-emerald-700 font-medium text-xs">Matrix Digital Rain background toggled.</div>`;
        break;

      case "theme": {
        const mode = args[0] as ThemeMode;
        if (mode === "dark" || mode === "light" || mode === "auto") {
          this.themeManager.setMode(mode);
          responseHTML = `<div class="dark:text-emerald-400 text-emerald-700 font-medium text-xs">Theme switched to: <b>${mode}</b></div>`;
        } else {
          responseHTML = `<div class="dark:text-slate-300 text-slate-700 text-xs">Current theme mode: <b class="dark:text-cyan-400 text-blue-600">${this.themeManager.getMode()}</b>. Usage: <code class="dark:text-emerald-300 text-emerald-700 font-bold">theme [dark|light|auto]</code></div>`;
        }
        break;
      }

      case "font": {
        const sub = (args[0] || "").toLowerCase();
        const param = args.slice(1).join(" ").trim();
        const curSettings = this.fontManager.getSettings();

        const isCap = this.fontManager.isCurrentFontLigatureCapable();
        const fontName = curSettings.fontFamily === "custom" && curSettings.customFamily ? curSettings.customFamily : curSettings.fontFamily;

        if (!sub) {
          responseHTML = `
            <div class="text-xs space-y-1.5 my-1 font-mono">
              <div class="font-bold dark:text-cyan-300 text-blue-600">Terminal Typography Settings:</div>
              <div class="grid grid-cols-1 sm:grid-cols-2 gap-1.5 p-3 rounded-lg dark:bg-slate-800/40 bg-slate-100 dark:border-slate-700 border-slate-200 border">
                <div><span class="dark:text-slate-400 text-slate-500">Family:</span> <b class="dark:text-emerald-400 text-emerald-700">${escapeHtml(fontName)}</b></div>
                <div><span class="dark:text-slate-400 text-slate-500">Size:</span> <b class="dark:text-emerald-400 text-emerald-700">${curSettings.fontSize}px</b></div>
                <div><span class="dark:text-slate-400 text-slate-500">Weight:</span> <b class="dark:text-emerald-400 text-emerald-700">${curSettings.fontWeight}</b></div>
                <div><span class="dark:text-slate-400 text-slate-500">Line Height:</span> <b class="dark:text-emerald-400 text-emerald-700">${curSettings.lineHeight}</b></div>
                <div><span class="dark:text-slate-400 text-slate-500">Letter Spacing:</span> <b class="dark:text-emerald-400 text-emerald-700">${curSettings.letterSpacing}px</b></div>
                <div><span class="dark:text-slate-400 text-slate-500">Ligatures:</span> <b class="${curSettings.ligatures && isCap ? "dark:text-emerald-400 text-emerald-700" : "text-slate-500"}">${!isCap ? "NOT SUPPORTED BY FONT" : curSettings.ligatures ? "ON" : "OFF"}</b></div>
              </div>
              ${!isCap ? `<div class="dark:text-amber-300 text-amber-700 text-[11px]">⚠️ <b>${escapeHtml(fontName)}</b> does not contain coding ligatures. Use <code class="dark:text-cyan-300 text-blue-600">font family "Fira Code"</code> or <code class="dark:text-cyan-300 text-blue-600">"JetBrains Mono"</code> for ligatures.</div>` : ""}
              <div class="dark:text-slate-400 text-slate-500 text-[11px]">
                Usage: <code class="dark:text-cyan-300 text-blue-600">font size &lt;px&gt;</code> | <code class="dark:text-cyan-300 text-blue-600">font family &lt;name&gt;</code> | <code class="dark:text-cyan-300 text-blue-600">font weight &lt;300..700&gt;</code> | <code class="dark:text-cyan-300 text-blue-600">font ligatures [on|off]</code> | <code class="dark:text-cyan-300 text-blue-600">font reset</code>
              </div>
            </div>
          `;
        } else if (sub === "size") {
          if (param === "+" || param === "inc") {
            const newSize = this.fontManager.increaseFontSize(1);
            responseHTML = `<div class="dark:text-emerald-400 text-emerald-700 font-medium text-xs">Font size increased to: <b>${newSize}px</b></div>`;
          } else if (param === "-" || param === "dec") {
            const newSize = this.fontManager.decreaseFontSize(1);
            responseHTML = `<div class="dark:text-emerald-400 text-emerald-700 font-medium text-xs">Font size decreased to: <b>${newSize}px</b></div>`;
          } else {
            const parsed = parseInt(param, 10);
            if (!isNaN(parsed) && parsed >= 8 && parsed <= 48) {
              this.fontManager.setFontSize(parsed);
              responseHTML = `<div class="dark:text-emerald-400 text-emerald-700 font-medium text-xs">Font size set to: <b>${parsed}px</b></div>`;
            } else {
              responseHTML = `<div class="dark:text-red-400 text-red-600 text-xs">Invalid font size. Please specify a number between 8 and 48 (e.g. <code>font size 16</code>).</div>`;
            }
          }
        } else if (sub === "family" || sub === "fontfamily") {
          if (param) {
            const clean = param.replace(/^["']|["']$/g, "").trim();
            this.fontManager.setFontFamily(clean);
            const supports = this.fontManager.doesFontSupportLigatures(clean);
            responseHTML = `
              <div class="text-xs my-0.5 space-y-1">
                <div class="dark:text-emerald-400 text-emerald-700 font-medium">Font family switched to: <b>${escapeHtml(clean)}</b></div>
                ${supports ? `<div class="dark:text-cyan-300 text-blue-600 text-[11px]">✓ This font supports programming ligatures (=== !== =&gt; &lt;= != &lt;!--).</div>` : `<div class="dark:text-amber-300 text-amber-700 text-[11px]">ℹ️ <b>${escapeHtml(clean)}</b> does not contain coding ligatures. For ligatures, choose Fira Code, JetBrains Mono, Cascadia Code, or Victor Mono.</div>`}
              </div>
            `;
          } else {
            responseHTML = `<div class="dark:text-slate-300 text-slate-700 text-xs">Usage: <code class="dark:text-emerald-300 text-emerald-700">font family &lt;Fira Code | JetBrains Mono | Cascadia Code | Victor Mono | Source Code Pro | Roboto Mono | Inconsolata | Space Mono | custom-name&gt;</code></div>`;
          }
        } else if (sub === "weight") {
          const w = parseInt(param, 10);
          if (!isNaN(w) && w >= 100 && w <= 900) {
            this.fontManager.setFontWeight(w);
            responseHTML = `<div class="dark:text-emerald-400 text-emerald-700 font-medium text-xs">Font weight set to: <b>${w}</b></div>`;
          } else {
            responseHTML = `<div class="dark:text-red-400 text-red-600 text-xs">Usage: <code class="dark:text-emerald-300 text-emerald-700">font weight &lt;300|400|500|600|700&gt;</code></div>`;
          }
        } else if (sub === "line-height" || sub === "lh") {
          const lh = parseFloat(param);
          if (!isNaN(lh) && lh >= 0.8 && lh <= 3.0) {
            this.fontManager.setLineHeight(lh);
            responseHTML = `<div class="dark:text-emerald-400 text-emerald-700 font-medium text-xs">Line height set to: <b>${lh}</b></div>`;
          } else {
            responseHTML = `<div class="dark:text-red-400 text-red-600 text-xs">Usage: <code class="dark:text-emerald-300 text-emerald-700">font line-height &lt;1.0..2.4&gt;</code></div>`;
          }
        } else if (sub === "letter-spacing" || sub === "spacing") {
          const sp = parseFloat(param);
          if (!isNaN(sp) && sp >= -2.0 && sp <= 6.0) {
            this.fontManager.setLetterSpacing(sp);
            responseHTML = `<div class="dark:text-emerald-400 text-emerald-700 font-medium text-xs">Letter spacing set to: <b>${sp}px</b></div>`;
          } else {
            responseHTML = `<div class="dark:text-red-400 text-red-600 text-xs">Usage: <code class="dark:text-emerald-300 text-emerald-700">font letter-spacing &lt;-1.0..4.0&gt;</code></div>`;
          }
        } else if (sub === "ligatures" || sub === "liga") {
          if (param === "on" || param === "true" || param === "1") {
            this.fontManager.setLigatures(true);
            responseHTML = `
              <div class="text-xs my-0.5 space-y-1">
                <div class="dark:text-emerald-400 text-emerald-700 font-medium">Font ligatures: <b>ENABLED</b></div>
                ${!isCap ? `<div class="dark:text-amber-300 text-amber-700 text-[11px]">⚠️ Note: Current font <b>${escapeHtml(fontName)}</b> does not contain ligature glyphs. Switch to Fira Code, JetBrains Mono, Cascadia Code, or Victor Mono to see ligatures.</div>` : `<div class="dark:text-cyan-300 text-blue-600 text-[11px]">Sample glyphs: <span class="font-bold font-mono">=== !== =&gt; -&gt; &lt;= &gt;= != &lt;!-- --&gt;</span></div>`}
              </div>
            `;
          } else if (param === "off" || param === "false" || param === "0") {
            this.fontManager.setLigatures(false);
            responseHTML = `<div class="dark:text-slate-400 text-slate-500 font-medium text-xs">Font ligatures: <b>DISABLED</b></div>`;
          } else {
            const toggled = this.fontManager.toggleLigatures();
            responseHTML = `
              <div class="text-xs my-0.5 space-y-1">
                <div class="dark:text-emerald-400 text-emerald-700 font-medium">Font ligatures toggled: <b>${toggled ? "ENABLED" : "DISABLED"}</b></div>
                ${toggled && !isCap ? `<div class="dark:text-amber-300 text-amber-700 text-[11px]">⚠️ Note: Current font <b>${escapeHtml(fontName)}</b> does not contain ligature glyphs. Switch to Fira Code, JetBrains Mono, Cascadia Code, or Victor Mono.</div>` : ""}
              </div>
            `;
          }
        } else if (sub === "reset") {
          this.fontManager.resetDefaults();
          responseHTML = `<div class="dark:text-emerald-400 text-emerald-700 font-medium text-xs">Font settings reset to defaults (Fira Code, 14px, 400, 1.5 lh, ligatures ON).</div>`;
        } else {
          responseHTML = `<div class="dark:text-red-400 text-red-600 text-xs">Unknown font subcommand: <code>${escapeHtml(sub)}</code>. Run <code>font</code> or <code>help</code> for options.</div>`;
        }
        break;
      }

      case "vim":
        this.vimEngine.toggle();
        tab.vimMode = this.vimEngine.getIsEnabled();
        tab.vimState = this.vimEngine.getMode();
        responseHTML = `<div class="dark:text-amber-300 text-amber-700 font-medium text-xs">Vim navigation mode: <b>${tab.vimMode ? "ENABLED (Press Esc for Normal mode)" : "DISABLED"}</b></div>`;
        break;

      case "stack":
        responseHTML = `<div class="text-xs space-y-1 my-1 font-mono">
          <div class="dark:text-cyan-300 text-blue-600 font-bold">Twominal Engine Architecture:</div>
          <div class="dark:text-slate-300 text-slate-700">🦀 Rust + Tauri backend with hardware-accelerated WebGPU/Canvas rendering and robust PTY session management.</div>
        </div>`;
        break;

      case "ligatures": {
        const isLig = this.fontManager.toggleLigatures();
        const isCap = this.fontManager.isCurrentFontLigatureCapable();
        const fontName = this.fontManager.getSettings().fontFamily;

        responseHTML = `
          <div class="text-xs my-1 space-y-1">
            <span class="dark:text-emerald-400 text-emerald-700 font-medium">Font Ligatures: <b>${isLig ? "ENABLED" : "DISABLED"}</b></span>
            ${isLig && !isCap ? `<div class="dark:text-amber-300 text-amber-700 text-[11px]">⚠️ Note: Current font <b>${escapeHtml(fontName)}</b> does not contain ligature glyphs. Switch to Fira Code, JetBrains Mono, Cascadia Code, or Victor Mono to view ligatures.</div>` : ""}
            <div class="dark:text-slate-300 text-slate-700 text-[11px]">Sample glyph test: <span class="dark:text-cyan-300 text-blue-600 font-bold font-mono">=== !== &lt;= &gt;= =&gt; -&gt; &lt;!-- --&gt; &amp;&amp; || ===&gt; != &lt;~&gt;</span></div>
          </div>
        `;
        break;
      }

      case "neofetch":
      case "twominalfetch": {
        try {
          const info = await invoke<SystemInfo>("get_system_info", { cwd: tab.cwd });
          const stack = "Rust + Tauri / GPUI";
          const isVim = this.vimEngine.getIsEnabled();
          const fontSet = this.fontManager.getSettings();
          const isCap = this.fontManager.isCurrentFontLigatureCapable();
          const fontDisp = `${fontSet.fontFamily === "custom" && fontSet.customFamily ? fontSet.customFamily : fontSet.fontFamily} @ ${fontSet.fontSize}px (${!isCap ? "No Ligatures" : fontSet.ligatures ? "Ligatures: On" : "Ligatures: Off"})`;

          responseHTML = `
            <div class="flex flex-col sm:flex-row items-start gap-4 text-xs font-mono my-2 p-3.5 rounded-xl dark:bg-slate-800/30 bg-slate-100 dark:border-slate-700/60 border-slate-200 border">
              <div class="dark:text-cyan-400 text-blue-600 font-bold leading-none select-none text-[11px]">
                <pre>
   ______                           
  /_  __/      ______  ____ ___  _  __
   / / | | /| / / __ \\/ __ \`__ \\/ / / /
  / /  | |/ |/ / /_/ / / / / / / /_/ / 
 /_/   |__/|__/\\____/_/ /_/ /_/\\__,_/  
                </pre>
              </div>
              <div class="space-y-1 dark:text-slate-300 text-slate-700">
                <div><span class="dark:text-cyan-300 text-blue-600 font-bold">${escapeHtml(info.user)}</span>@<span class="dark:text-indigo-400 text-indigo-700 font-bold">${escapeHtml(info.host)}</span></div>
                <div class="dark:text-slate-600 text-slate-300">--------------------------</div>
                <div><b class="dark:text-slate-400 text-slate-500">OS:</b> ${escapeHtml(info.os)} (${escapeHtml(info.arch)})</div>
                <div><b class="dark:text-slate-400 text-slate-500">Host:</b> WebGPU High-Performance Canvas</div>
                <div><b class="dark:text-slate-400 text-slate-500">Kernel:</b> ${escapeHtml(info.kernel)}</div>
                <div><b class="dark:text-slate-400 text-slate-500">Shell:</b> Twominal Fish Shell 3.8.0 (${escapeHtml(info.shell)})</div>
                <div><b class="dark:text-slate-400 text-slate-500">Stack:</b> <span class="dark:text-cyan-300 text-blue-600 font-semibold">${escapeHtml(stack)}</span></div>
                <div><b class="dark:text-slate-400 text-slate-500">Font:</b> <span class="dark:text-emerald-400 text-emerald-700 font-medium">${escapeHtml(fontDisp)}</span></div>
                <div><b class="dark:text-slate-400 text-slate-500">Vim Mode:</b> ${isVim ? '<span class="dark:text-amber-400 text-amber-700 font-medium">Enabled</span>' : '<span class="dark:text-slate-500 text-slate-400">Disabled</span>'}</div>
                <div class="flex items-center gap-1.5 pt-1">
                  <span class="w-3 h-3 rounded-full bg-red-500 inline-block"></span>
                  <span class="w-3 h-3 rounded-full bg-amber-500 inline-block"></span>
                  <span class="w-3 h-3 rounded-full bg-emerald-500 inline-block"></span>
                  <span class="w-3 h-3 rounded-full bg-blue-500 inline-block"></span>
                  <span class="w-3 h-3 rounded-full bg-indigo-500 inline-block"></span>
                  <span class="w-3 h-3 rounded-full bg-pink-500 inline-block"></span>
                </div>
              </div>
            </div>
          `;
        } catch {
          responseHTML = `<div class="dark:text-slate-300 text-slate-700 text-xs">Twominal Shell v1.0.4-hybrid</div>`;
        }
        break;
      }

      case "history": {
        const hist = this.historyManager.getAll();
        responseHTML = `
          <div class="text-xs dark:text-slate-300 text-slate-700 space-y-0.5 my-1 max-h-60 overflow-y-auto font-mono">
            ${hist.map((h, i) => `<div><span class="dark:text-slate-500 text-slate-400 w-8 inline-block">${i + 1}</span> ${escapeHtml(h)}</div>`).join("")}
          </div>
        `;
        break;
      }

      case "date":
        responseHTML = `<div class="dark:text-slate-300 text-slate-700 text-xs">${new Date().toString()}</div>`;
        break;

      case "settings":
      case "config":
      case "options": {
        document.getElementById("btn-open-settings")?.click();
        responseHTML = `<div class="dark:text-cyan-400 text-blue-600 font-medium text-xs">Opened Settings &amp; Typography configuration dialog.</div>`;
        break;
      }

      case "tabs":
      case "tab": {
        const sub = (args[0] || "").toLowerCase();
        if (sub === "new" || sub === "create" || sub === "add") {
          if (this.tabManager) {
            await this.tabManager.createNewTab();
          } else {
            document.getElementById("btn-new-tab")?.click();
          }
          responseHTML = `<div class="dark:text-emerald-400 text-emerald-700 font-medium text-xs">New terminal tab opened.</div>`;
        } else if (sub === "close" || sub === "kill") {
          if (this.tabManager) {
            this.tabManager.closeActiveTab();
          }
          return;
        } else if (sub === "next") {
          this.tabManager?.switchToNextTab();
          return;
        } else if (sub === "prev" || sub === "previous") {
          this.tabManager?.switchToPrevTab();
          return;
        } else {
          const tabList = this.tabManager?.getTabs() || [];
          responseHTML = `
            <div class="text-xs space-y-1 my-1 font-mono">
              <div class="font-bold dark:text-cyan-300 text-blue-600">Open Terminal Tabs (${tabList.length}):</div>
              ${tabList.map((t, idx) => `
                <div class="dark:text-slate-300 text-slate-700 flex items-center gap-2">
                  <span class="font-bold ${t.id === tab.id ? 'dark:text-emerald-400 text-emerald-600' : 'text-slate-500'}">[${idx + 1}]</span>
                  <span class="${t.id === tab.id ? 'font-bold' : ''}">${escapeHtml(t.title)}</span>
                  ${t.id === tab.id ? '<span class="text-[10px] px-1 rounded bg-emerald-500/20 text-emerald-400">ACTIVE</span>' : ''}
                </div>
              `).join("")}
              <div class="dark:text-slate-400 text-slate-500 text-[11px] mt-1">Usage: <code class="dark:text-cyan-300 text-blue-600">/tabs new</code> | <code class="dark:text-cyan-300 text-blue-600">/tabs close</code> | <code class="dark:text-cyan-300 text-blue-600">/tabs next</code> | <code class="dark:text-cyan-300 text-blue-600">/tabs prev</code></div>
            </div>
          `;
        }
        break;
      }

      case "exit":
      case "quit": {
        if (this.tabManager) {
          const tabs = this.tabManager.getTabs();
          if (tabs.length > 1) {
            this.tabManager.closeActiveTab();
            return;
          }
        }
        tab.outputHistory = [];
        return;
      }

      case "cd": {
        // Direct CD execution
        try {
          const res = await invoke<ShellExecResult>("shell_exec", {
            tabId: tab.id,
            cwd: tab.cwd || ".",
            command: cmd,
          });
          if (res.new_cwd) {
            tab.cwd = res.new_cwd;
            tab.displayCwd = res.display_cwd || res.new_cwd;
          }
          tab.gitBranch = res.git_branch || "";
          if (res.stderr) {
            responseHTML = `<div class="text-xs my-0.5 dark:text-red-400 text-red-600 font-mono">${escapeHtml(res.stderr)}</div>`;
          }
        } catch (err: any) {
          responseHTML = `<div class="text-xs my-0.5 dark:text-red-400 text-red-600 font-mono">cd: ${escapeHtml(String(err))}</div>`;
        }
        break;
      }

      case "pty": {
        const ptyCmd = args.join(" ").trim();
        if (!ptyCmd) {
          responseHTML = `<div class="dark:text-slate-300 text-slate-700 text-xs font-mono">Usage: <code class="dark:text-cyan-300 text-blue-600">/pty &lt;command&gt;</code> to run a process directly in raw PTY mode.</div>`;
          break;
        }
        const ptyProc = args[0]?.toLowerCase() || "pty";
        await this.runPtyCommand(tab, ptyCmd, ptyProc);
        return;
      }

      default: {
        if (isInteractiveCommand(mainCmd, args)) {
          await this.runPtyCommand(tab, cmd, mainCmd);
          return;
        }

        // Standard execution with live streaming output
        tab.activeProcess = mainCmd;
        this.onTabUpdateCallback?.();
        this.onRenderCallback?.();

        let streamingText = "";
        let unlisten: (() => void) | undefined;
        try {
          unlisten = await listen<{ tab_id: string; data: string }>(
            "shell-output",
            (event) => {
              if (event.payload.tab_id === tab.id) {
                streamingText += event.payload.data;
                tab.activeStreamText = streamingText;
                this.onRenderCallback?.();
              }
            }
          );
        } catch {
          // Ignore
        }

        try {
          const res = await invoke<ShellExecResult>("shell_exec", {
            tabId: tab.id,
            cwd: tab.cwd || ".",
            command: cmd,
          });

          if (unlisten) unlisten();
          tab.activeStreamText = undefined;

          if (res.new_cwd) {
            tab.cwd = res.new_cwd;
            tab.displayCwd = res.display_cwd || res.new_cwd;
          }
          if (res.git_branch !== undefined) {
            tab.gitBranch = res.git_branch || "";
          }

          let output = "";
          if (res.stdout && res.stderr) {
            output = res.stdout.endsWith("\n") ? res.stdout + res.stderr : res.stdout + "\n" + res.stderr;
          } else {
            output = res.stdout || res.stderr || "";
          }

          if (output) {
            responseHTML = `<div class="font-mono text-xs sm:text-sm whitespace-pre-wrap leading-relaxed select-text py-0.5">${ansiToHtml(output)}</div>`;
          } else if (res.exit_code !== 0) {
            responseHTML = `<div class="font-mono text-xs sm:text-sm text-red-400 py-0.5">[Process exited with code ${res.exit_code}]</div>`;
          }
        } catch (err: any) {
          if (unlisten) unlisten();
          tab.activeStreamText = undefined;
          responseHTML = `<div class="font-mono text-xs sm:text-sm text-red-400 py-0.5">${escapeHtml(String(err))}</div>`;
        }
        break;
      }
    }
    
    // Command finished - reset active process to shell
    tab.activeProcess = "fish";
    this.onTabUpdateCallback?.();

    if (responseHTML) {
      tab.outputHistory.push({ type: "response", html: responseHTML });
    }
  }

  private async runPtyCommand(tab: TabSessionData, cmd: string, processName: string): Promise<void> {
    tab.isPtyRunning = true;
    tab.activeProcess = processName;

    if (!tab.terminalSession) {
      tab.terminalSession = new TerminalSession(tab.id);
    }

    const ptyLayer = document.getElementById("terminal-pty-layer");
    const promptRow = document.getElementById("prompt-row");

    if (ptyLayer) {
      ptyLayer.classList.remove("hidden");
      tab.terminalSession.mount(ptyLayer);
    }

    if (promptRow) {
      promptRow.classList.add("hidden");
    }

    tab.terminalSession.onExit(async () => {
      tab.isPtyRunning = false;
      tab.activeProcess = "fish";

      const wasAlternate = tab.terminalSession?.wasAlternateBufferUsed() ?? false;

      if (ptyLayer) {
        ptyLayer.classList.add("hidden");
      }
      if (promptRow) {
        promptRow.classList.remove("hidden");
      }

      // If it was not a full-screen TUI (like vim/htop/nano), commit its output to scrollback history!
      if (!wasAlternate) {
        const serialized = tab.terminalSession?.getSerializedOutput() ?? "";
        const rawOut = tab.terminalSession?.getAccumulatedOutput() ?? "";
        const bufferText = tab.terminalSession?.getBufferText() ?? "";

        let htmlContent = "";
        // Prioritize serialized terminal buffer: it reflects the actual resolved screen state
        // with all cursor rewrites, progress updates, and erasures handled cleanly, exactly like native terminal.
        if (serialized.trim()) {
          htmlContent = ansiToHtml(serialized).trim();
        }
        if (!htmlContent && bufferText.trim()) {
          htmlContent = escapeHtml(bufferText.trim());
        }
        if (!htmlContent && rawOut.trim()) {
          htmlContent = ansiToHtml(rawOut).trim();
        }

        if (htmlContent) {
          tab.outputHistory.push({
            type: "response",
            html: `<div class="font-mono text-xs sm:text-sm whitespace-pre-wrap leading-relaxed select-text py-0.5">${htmlContent}</div>`,
          });
        }
      }

      // Refresh git branch & cwd
      try {
        const branch = await invoke<string | null>("get_git_branch", { cwd: tab.cwd });
        tab.gitBranch = branch || "";
      } catch {
        // Ignore
      }

      const cliInput = document.getElementById("cli-input") as HTMLInputElement;
      cliInput?.focus();

      this.onTabUpdateCallback?.();
      this.onRenderCallback?.();
    });

    this.onTabUpdateCallback?.();
    this.onRenderCallback?.();

    await tab.terminalSession.start(cmd, tab.cwd);
  }
}


import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { SerializeAddon } from "@xterm/addon-serialize";
import { invoke } from "@tauri-apps/api/core";
import { listen, UnlistenFn } from "@tauri-apps/api/event";
import { FontManager } from "../fonts/FontManager";
import "@xterm/xterm/css/xterm.css";

export class TerminalSession {
  private terminal: Terminal;
  private fitAddon: FitAddon;
  private serializeAddon: SerializeAddon;
  private container: HTMLElement | null = null;
  private tabId: string;
  private unlistenOutput?: UnlistenFn;
  private unlistenExit?: UnlistenFn;
  private isRunning: boolean = false;
  private onExitCallback?: () => void;
  private accumulatedOutput: string = "";
  private alternateBufferUsed: boolean = false;

  constructor(tabId: string) {
    this.tabId = tabId;
    this.fitAddon = new FitAddon();
    this.serializeAddon = new SerializeAddon();

    const fontSettings = FontManager.getInstance().getSettings();
    const isDark = document.documentElement.classList.contains("dark");

    this.terminal = new Terminal({
      cursorBlink: true,
      cursorStyle: "block",
      fontFamily:
        fontSettings.fontFamily === "custom" && fontSettings.customFamily
          ? fontSettings.customFamily
          : `"${fontSettings.fontFamily}", Menlo, Monaco, Consolas, monospace`,
      fontSize: fontSettings.fontSize || 14,
      fontWeight: (fontSettings.fontWeight as any) || 400,
      lineHeight: fontSettings.lineHeight || 1.4,
      letterSpacing: fontSettings.letterSpacing || 0,
      allowProposedApi: true,
      theme: this.getTerminalTheme(isDark),
      allowTransparency: true,
      scrollback: 5000,
      convertEol: true,
    });

    this.terminal.loadAddon(this.fitAddon);
    this.terminal.loadAddon(this.serializeAddon);

    // Forward user keystrokes directly to backend PTY
    this.terminal.onData((data) => {
      if (this.isRunning) {
        invoke("pty_write", { id: this.tabId, data }).catch(() => {});
      }
    });

    // Forward terminal resize events
    this.terminal.onResize(({ cols, rows }) => {
      if (this.isRunning) {
        invoke("pty_resize", { id: this.tabId, cols, rows }).catch(() => {});
      }
    });
  }

  public getTerminal(): Terminal {
    return this.terminal;
  }

  public getFitAddon(): FitAddon {
    return this.fitAddon;
  }

  public getIsRunning(): boolean {
    return this.isRunning;
  }

  public getContainer(): HTMLElement | null {
    return this.container;
  }

  public onExit(cb: () => void): void {
    this.onExitCallback = cb;
  }

  public mount(container: HTMLElement): void {
    this.container = container;
    if (!this.terminal.element) {
      container.innerHTML = "";
      this.terminal.open(container);
    } else if (this.terminal.element.parentElement !== container) {
      container.innerHTML = "";
      container.appendChild(this.terminal.element);
    }
    requestAnimationFrame(() => {
      this.fit();
    });
  }

  public async start(command?: string, cwd?: string): Promise<void> {
    this.isRunning = true;
    this.accumulatedOutput = "";
    this.alternateBufferUsed = false;
    this.terminal.reset();

    // Listen for PTY output
    this.unlistenOutput = await listen<{ id: string; data: string }>(
      "pty-output",
      (event) => {
        if (event.payload.id === this.tabId) {
          this.accumulatedOutput += event.payload.data;
          this.terminal.write(event.payload.data);
          if (
            this.terminal.buffer.active.type === "alternate" ||
            event.payload.data.includes("\x1b[?1049h") ||
            event.payload.data.includes("\x1b[?47h") ||
            event.payload.data.includes("\x1b[?1047h")
          ) {
            this.alternateBufferUsed = true;
          }
        }
      }
    );

    // Listen for PTY exit
    this.unlistenExit = await listen<{ id: string; exit_code?: number }>(
      "pty-exit",
      async (event) => {
        if (event.payload.id === this.tabId) {
          await new Promise((resolve) => setTimeout(resolve, 50));
          this.stop();
          this.onExitCallback?.();
        }
      }
    );

    this.fit();
    const cols = Math.max(80, this.terminal.cols || 100);
    const rows = Math.max(24, this.terminal.rows || 30);

    try {
      await invoke("pty_spawn", {
        id: this.tabId,
        cols,
        rows,
        cwd: cwd || ".",
        shell: null,
        command: command || null,
      });
    } catch (err) {
      this.terminal.writeln(`\r\n\x1b[31mtwominal: Error launching process: ${err}\x1b[0m\r\n`);
      this.stop();
      this.onExitCallback?.();
    }

    this.terminal.focus();
  }

  public stop(): void {
    this.isRunning = false;
    if (this.unlistenOutput) {
      this.unlistenOutput();
      this.unlistenOutput = undefined;
    }
    if (this.unlistenExit) {
      this.unlistenExit();
      this.unlistenExit = undefined;
    }
  }

  public focus(): void {
    this.terminal.focus();
  }

  public fit(): void {
    try {
      this.fitAddon.fit();
      const cols = this.terminal.cols;
      const rows = this.terminal.rows;
      if (this.isRunning && cols && rows) {
        invoke("pty_resize", { id: this.tabId, cols, rows }).catch(() => {});
      }
    } catch {
      // Ignore
    }
  }

  public updateTheme(isDark: boolean): void {
    this.terminal.options.theme = this.getTerminalTheme(isDark);
  }

  public updateFont(settings: any): void {
    this.terminal.options.fontFamily =
      settings.fontFamily === "custom" && settings.customFamily
        ? settings.customFamily
        : `"${settings.fontFamily}", Menlo, Monaco, Consolas, monospace`;
    this.terminal.options.fontSize = settings.fontSize || 14;
    this.terminal.options.fontWeight = settings.fontWeight || 400;
    this.terminal.options.lineHeight = settings.lineHeight || 1.4;
    this.terminal.options.letterSpacing = settings.letterSpacing || 0;
    this.fit();
  }

  private getTerminalTheme(isDark: boolean) {
    if (isDark) {
      return {
        background: "#0d1117",
        foreground: "#cdd6f4",
        cursor: "#89dceb",
        cursorAccent: "#11111b",
        selectionBackground: "rgba(137, 180, 250, 0.3)",
        black: "#1e1e2e",
        red: "#f38ba8",
        green: "#a6e3a1",
        yellow: "#f9e2af",
        blue: "#89b4fa",
        magenta: "#cba6f7",
        cyan: "#89dceb",
        white: "#cdd6f4",
        brightBlack: "#585b70",
        brightRed: "#ff5555",
        brightGreen: "#50fa7b",
        brightYellow: "#f1fa8c",
        brightBlue: "#bd93f9",
        brightMagenta: "#ff79c6",
        brightCyan: "#8be9fd",
        brightWhite: "#ffffff",
      };
    } else {
      return {
        background: "#ffffff",
        foreground: "#1e293b",
        cursor: "#0284c7",
        cursorAccent: "#ffffff",
        selectionBackground: "rgba(2, 132, 199, 0.25)",
        black: "#0f172a",
        red: "#e11d48",
        green: "#16a34a",
        yellow: "#ca8a04",
        blue: "#2563eb",
        magenta: "#9333ea",
        cyan: "#0891b2",
        white: "#f8fafc",
        brightBlack: "#64748b",
        brightRed: "#f43f5e",
        brightGreen: "#22c55e",
        brightYellow: "#eab308",
        brightBlue: "#3b82f6",
        brightMagenta: "#a855f7",
        brightCyan: "#06b6d4",
        brightWhite: "#ffffff",
      };
    }
  }

  public getSerializedOutput(): string {
    try {
      return this.serializeAddon.serialize();
    } catch {
      return "";
    }
  }

  public getAccumulatedOutput(): string {
    return this.accumulatedOutput;
  }

  public wasAlternateBufferUsed(): boolean {
    return this.alternateBufferUsed || this.terminal.buffer.active.type === "alternate";
  }

  public getBufferText(): string {
    try {
      const buf = this.terminal.buffer.normal;
      const lines: string[] = [];
      for (let i = 0; i < buf.length; i++) {
        const line = buf.getLine(i);
        if (line) {
          lines.push(line.translateToString(true));
        }
      }
      while (lines.length > 0 && lines[lines.length - 1].trim() === "") {
        lines.pop();
      }
      return lines.join("\n");
    } catch {
      return "";
    }
  }

  public destroy(): void {
    this.stop();
    this.terminal.dispose();
  }
}

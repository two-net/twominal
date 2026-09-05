import { FitAddon } from "@xterm/addon-fit";
import { Unicode11Addon } from "@xterm/addon-unicode11";
import { Terminal, type IDisposable } from "@xterm/xterm";
import type { TerminalConfig } from "../config/types";
import type { ResolvedAppearance } from "../theme";
import {
  ShellExperienceController,
  type HistoryEntry,
  type ShellClient,
  type ShellExperienceStatus,
} from "../shell";
import { encodeBinaryData, encodeTerminalData } from "./bytes";
import { InputQueue } from "./InputQueue";
import { findLigatureRanges } from "./ligatures";
import {
  TauriTerminalClient,
  type TerminalClient,
} from "./terminalClient";
import { terminalTheme } from "./themes";
import type {
  SessionDescriptor,
  SessionLifecycleEvent,
  TerminalSize,
  TerminalViewState,
} from "./types";

const ACK_BATCH_BYTES = 32 * 1024;
const ACK_DELAY_MILLISECONDS = 8;

export interface TerminalControllerOptions {
  appearance: ResolvedAppearance;
  active: boolean;
  terminalConfig: TerminalConfig;
  vimMode: boolean;
  onStateChange: (state: TerminalViewState) => void;
  onTitleChange?: (title: string) => void;
  history?: readonly HistoryEntry[];
  onCommandAccepted?: (command: string) => void;
  onShellExperienceChange?: (status: ShellExperienceStatus) => void;
  shellClient?: ShellClient;
  client?: TerminalClient;
  transferToken?: string;
}

export class TerminalController {
  private readonly terminal: Terminal;
  private readonly fitAddon = new FitAddon();
  private readonly unicodeAddon = new Unicode11Addon();
  private readonly client: TerminalClient;
  private readonly disposables: IDisposable[] = [];
  private readonly inputQueue: InputQueue;
  private resizeObserver: ResizeObserver | null = null;
  private resizeFrame: number | null = null;
  private pendingResize: TerminalSize | null = null;
  private resizing = false;
  private session: SessionDescriptor | null = null;
  private disposed = false;
  private active: boolean;
  private sessionEnded = false;
  private pendingAcknowledgement = 0;
  private acknowledgementTimer: ReturnType<typeof setTimeout> | null = null;
  private acknowledgementChain: Promise<void> = Promise.resolve();
  private pendingLifecycle: SessionLifecycleEvent[] = [];
  private ligatureJoinerId: number | null = null;
  private shellExperience: ShellExperienceController | null = null;

  constructor(
    private readonly host: HTMLElement,
    private readonly options: TerminalControllerOptions,
  ) {
    this.client = options.client ?? new TauriTerminalClient();
    this.active = options.active;
    this.terminal = new Terminal({
      allowProposedApi: true,
      allowTransparency: false,
      convertEol: false,
      cursorBlink: true,
      cursorStyle: "block",
      fontFamily: options.terminalConfig.fontFamily,
      fontSize: options.terminalConfig.fontSize,
      lineHeight: options.terminalConfig.lineHeight,
      letterSpacing: options.terminalConfig.letterSpacing,
      fontWeight: options.terminalConfig.fontWeight,
      fontWeightBold: boldFontWeight(options.terminalConfig.fontWeight),
      scrollback: 10_000,
      theme: terminalTheme(options.appearance),
    });
    this.inputQueue = new InputQueue(
      async (data) => {
        if (!this.session || this.sessionEnded) {
          return;
        }
        await this.client.write(this.session.sessionId, data);
      },
      (error) => this.reportTransportError(error),
      {
        isRetryable: (error) =>
          commandErrorCode(error) === "input_backpressure",
      },
    );
  }

  async start(): Promise<void> {
    this.options.onStateChange({ type: "starting" });
    try {
      this.terminal.loadAddon(this.fitAddon);
      this.terminal.loadAddon(this.unicodeAddon);
      this.terminal.unicode.activeVersion = "11";
      this.terminal.open(this.host);
      this.applyTerminalHostTypography(this.options.terminalConfig);
      this.applyLigatureSetting(this.options.terminalConfig.fontLigatures);
      this.shellExperience = new ShellExperienceController({
        terminal: this.terminal,
        host: this.host,
        active: this.active,
        history: this.options.history,
        client: this.options.shellClient,
        vimMode: this.options.vimMode,
        sendData: (data) => {
          this.inputQueue.enqueue(encodeTerminalData(data));
        },
        onCommandAccepted: (command) =>
          this.options.onCommandAccepted?.(command),
        onStatusChange: (status) =>
          this.options.onShellExperienceChange?.(status),
      });

      this.disposables.push(
        this.terminal.onResize((size) => {
          this.pendingResize = { rows: size.rows, cols: size.cols };
          void this.drainResize();
        }),
        this.terminal.onTitleChange((title) => {
          this.options.onTitleChange?.(title);
        }),
      );
      this.installResizeObserver();

      await this.waitForLayout();
      if (this.disposed) {
        return;
      }

      this.fit();
      void this.enableWebgl();

      const size = { rows: this.terminal.rows, cols: this.terminal.cols };
      const session = this.options.transferToken
        ? await this.client.attach(
            this.options.transferToken,
            size,
            (data) => this.handleSnapshot(data),
            (data) => this.handleOutput(data),
            (event) => this.handleLifecycle(event),
          )
        : await this.client.start(
            size,
            (data) => this.handleOutput(data),
            (event) => this.handleLifecycle(event),
          );

      if (this.disposed) {
        await this.client.close(session.sessionId).catch(() => undefined);
        return;
      }

      this.session = session;
      this.shellExperience.setSession(session);
      this.disposables.push(
        this.terminal.onData((data) => {
          if (!this.active) {
            return;
          }
          if (this.shellExperience?.handleTerminalData(data) ?? true) {
            this.inputQueue.enqueue(encodeTerminalData(data));
          }
        }),
        this.terminal.onBinary((data) => {
          if (!this.active) {
            return;
          }
          this.inputQueue.enqueue(encodeBinaryData(data));
        }),
      );

      this.options.onStateChange({ type: "running", session });
      this.flushAcknowledgement();
      this.fit();
      void this.drainResize();
      if (this.active) {
        this.terminal.focus();
      }

      for (const event of this.pendingLifecycle.splice(0)) {
        this.handleLifecycle(event);
      }
    } catch (error) {
      if (!this.disposed) {
        this.sessionEnded = true;
        this.inputQueue.dispose();
        this.closeSession();
        this.options.onStateChange({
          type: "error",
          message: errorMessage(error, "Unable to start the terminal"),
        });
      }
    }
  }

  applyAppearance(appearance: ResolvedAppearance): void {
    this.terminal.options.theme = terminalTheme(appearance);
  }

  applyTerminalConfig(config: TerminalConfig): void {
    this.terminal.options.fontFamily = config.fontFamily;
    this.terminal.options.fontSize = config.fontSize;
    this.terminal.options.lineHeight = config.lineHeight;
    this.terminal.options.letterSpacing = config.letterSpacing;
    this.terminal.options.fontWeight = config.fontWeight;
    this.terminal.options.fontWeightBold = boldFontWeight(config.fontWeight);
    this.applyTerminalHostTypography(config);
    this.applyLigatureSetting(config.fontLigatures);
    this.fitAfterFontsLoad();
  }

  applyVimMode(enabled: boolean): void {
    this.shellExperience?.setVimMode(enabled);
  }

  updateHistory(history: readonly HistoryEntry[]): void {
    this.shellExperience?.updateHistory(history);
  }

  setActive(active: boolean): void {
    this.active = active;
    this.shellExperience?.setActive(active);
    if (active && !this.disposed) {
      this.fit();
      this.terminal.focus();
    }
  }

  async dispose(): Promise<void> {
    if (this.disposed) {
      return;
    }
    this.disposed = true;
    this.shellExperience?.dispose();
    this.shellExperience = null;
    this.inputQueue.dispose();
    this.resizeObserver?.disconnect();
    this.resizeObserver = null;
    if (this.resizeFrame !== null) {
      cancelAnimationFrame(this.resizeFrame);
      this.resizeFrame = null;
    }
    if (this.acknowledgementTimer !== null) {
      clearTimeout(this.acknowledgementTimer);
      this.acknowledgementTimer = null;
    }
    for (const disposable of this.disposables.splice(0)) {
      disposable.dispose();
    }
    if (this.ligatureJoinerId !== null) {
      this.terminal.deregisterCharacterJoiner(this.ligatureJoinerId);
      this.ligatureJoinerId = null;
    }

    const sessionId = this.takeSessionId();
    this.terminal.dispose();
    if (sessionId) {
      await this.client.close(sessionId).catch(() => undefined);
    }
  }

  private async waitForLayout(): Promise<void> {
    this.fitAfterFontsLoad();
    await Promise.resolve();
  }

  private fitAfterFontsLoad(): void {
    void document.fonts?.ready
      .then(() => {
        if (!this.disposed) {
          this.fit();
        }
      })
      .catch(() => undefined);
    this.fit();
  }

  private applyLigatureSetting(enabled: boolean): void {
    const element = this.terminal.element;
    if (enabled && this.ligatureJoinerId === null) {
      this.ligatureJoinerId =
        this.terminal.registerCharacterJoiner(findLigatureRanges);
    } else if (!enabled && this.ligatureJoinerId !== null) {
      this.terminal.deregisterCharacterJoiner(this.ligatureJoinerId);
      this.ligatureJoinerId = null;
    }

    if (element) {
      element.style.fontFeatureSettings = enabled
        ? '"calt" 1, "liga" 1'
        : '"calt" 0, "liga" 0';
    }
    if (!this.disposed && this.terminal.rows > 0) {
      this.terminal.refresh(0, this.terminal.rows - 1);
    }
  }

  private applyTerminalHostTypography(config: TerminalConfig): void {
    this.host.style.setProperty("--terminal-font-family", config.fontFamily);
    this.host.style.setProperty("--terminal-font-size", `${config.fontSize}px`);
    this.host.style.setProperty(
      "--terminal-letter-spacing",
      `${config.letterSpacing}px`,
    );
    this.host.style.setProperty(
      "--terminal-font-weight",
      String(config.fontWeight),
    );
  }

  private installResizeObserver(): void {
    if (typeof ResizeObserver === "undefined") {
      return;
    }

    this.resizeObserver = new ResizeObserver(() => {
      if (this.resizeFrame !== null) {
        return;
      }
      this.resizeFrame = requestAnimationFrame(() => {
        this.resizeFrame = null;
        this.fit();
      });
    });
    this.resizeObserver.observe(this.host);
  }

  private fit(): void {
    if (
      this.disposed ||
      this.host.clientWidth === 0 ||
      this.host.clientHeight === 0
    ) {
      return;
    }

    const dimensions = this.fitAddon.proposeDimensions();
    if (
      dimensions &&
      dimensions.cols > 0 &&
      dimensions.rows > 0 &&
      (dimensions.cols !== this.terminal.cols ||
        dimensions.rows !== this.terminal.rows)
    ) {
      this.terminal.resize(dimensions.cols, dimensions.rows);
    }
  }

  private async enableWebgl(): Promise<void> {
    try {
      const { WebglAddon } = await import("@xterm/addon-webgl");
      if (this.disposed) {
        return;
      }
      const webglAddon = new WebglAddon();
      this.terminal.loadAddon(webglAddon);
      const contextLoss = webglAddon.onContextLoss(() => {
        contextLoss.dispose();
        webglAddon.dispose();
        if (!this.disposed && this.terminal.rows > 0) {
          this.terminal.refresh(0, this.terminal.rows - 1);
        }
      });
      this.disposables.push(contextLoss, webglAddon);
    } catch {
      // xterm's built-in renderer remains active when WebGL is unavailable.
    }
  }

  private handleOutput(data: ArrayBuffer): void {
    if (this.disposed || data.byteLength === 0) {
      return;
    }

    const bytes = new Uint8Array(data);
    this.terminal.write(bytes, () => {
      if (this.disposed) {
        return;
      }
      this.pendingAcknowledgement += bytes.byteLength;
      if (this.pendingAcknowledgement >= ACK_BATCH_BYTES) {
        this.flushAcknowledgement();
      } else if (this.acknowledgementTimer === null) {
        this.acknowledgementTimer = setTimeout(() => {
          this.acknowledgementTimer = null;
          this.flushAcknowledgement();
        }, ACK_DELAY_MILLISECONDS);
      }
    });
  }

  private handleSnapshot(data: ArrayBuffer): void {
    if (this.disposed || data.byteLength === 0) {
      return;
    }
    this.terminal.write(new Uint8Array(data));
  }

  private flushAcknowledgement(): void {
    if (!this.session || this.pendingAcknowledgement === 0 || this.disposed) {
      return;
    }
    if (this.acknowledgementTimer !== null) {
      clearTimeout(this.acknowledgementTimer);
      this.acknowledgementTimer = null;
    }

    const bytes = this.pendingAcknowledgement;
    const sessionId = this.session.sessionId;
    this.pendingAcknowledgement = 0;
    this.acknowledgementChain = this.acknowledgementChain
      .then(() => this.client.acknowledgeOutput(sessionId, bytes))
      .catch((error: unknown) => {
        if (!this.sessionEnded) {
          this.reportTransportError(error);
        }
      });
  }

  private handleLifecycle(event: SessionLifecycleEvent): void {
    if (this.disposed || this.sessionEnded) {
      return;
    }
    if (!this.session) {
      this.pendingLifecycle.push(event);
      return;
    }

    if (event.type === "exited") {
      this.sessionEnded = true;
      this.shellExperience?.setActive(false);
      this.inputQueue.dispose();
      this.options.onStateChange({
        type: "exited",
        session: this.session,
        exitCode: event.exitCode,
        signal: event.signal,
      });
      return;
    }

    this.sessionEnded = true;
    this.shellExperience?.setActive(false);
    this.inputQueue.dispose();
    this.closeSession();
    this.options.onStateChange({
      type: "error",
      message: errorMessage(event.message, "Terminal connection failed"),
    });
  }

  private async drainResize(): Promise<void> {
    if (this.resizing || !this.session || this.disposed || this.sessionEnded) {
      return;
    }
    this.resizing = true;
    try {
      while (this.pendingResize && this.session && !this.disposed) {
        const size = this.pendingResize;
        this.pendingResize = null;
        await this.client.resize(this.session.sessionId, size);
      }
    } catch (error) {
      this.reportTransportError(error);
    } finally {
      this.resizing = false;
      if (this.pendingResize) {
        void this.drainResize();
      }
    }
  }

  private reportTransportError(error: unknown): void {
    if (this.disposed || this.sessionEnded) {
      return;
    }
    this.sessionEnded = true;
    this.shellExperience?.setActive(false);
    this.inputQueue.dispose();
    this.closeSession();
    this.options.onStateChange({
      type: "error",
      message: errorMessage(error, "Terminal connection failed"),
    });
  }

  private takeSessionId(): string | null {
    const sessionId = this.session?.sessionId ?? null;
    this.session = null;
    return sessionId;
  }

  private closeSession(): void {
    const sessionId = this.takeSessionId();
    if (sessionId) {
      void this.client.close(sessionId).catch(() => undefined);
    }
  }
}

function errorMessage(error: unknown, fallback: string): string {
  if (error instanceof Error && error.message.trim()) {
    return error.message.slice(0, 500);
  }
  if (typeof error === "string" && error.trim()) {
    return error.slice(0, 500);
  }
  if (typeof error === "object" && error !== null && "message" in error) {
    const message = error.message;
    if (typeof message === "string" && message.trim()) {
      return message.slice(0, 500);
    }
  }
  return fallback;
}

function boldFontWeight(weight: number): number {
  return Math.min(900, weight + 200);
}

function commandErrorCode(error: unknown): string | null {
  if (typeof error === "object" && error !== null && "code" in error) {
    return typeof error.code === "string" ? error.code : null;
  }
  if (typeof error === "string" && error.startsWith("{")) {
    try {
      return commandErrorCode(JSON.parse(error));
    } catch {
      return null;
    }
  }
  return null;
}

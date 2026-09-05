import type { IDisposable, Terminal } from "@xterm/xterm";
import { bestAutosuggestion } from "./autosuggestions";
import {
  buildCompletionChoices,
  completionRequestForLine,
  syntaxContext,
  type CompletionChoice,
} from "./completions";
import {
  HistoryNavigator,
  type HistoryEntry,
  type HistorySource,
} from "./history";
import { PromptInputModel } from "./PromptInputModel";
import { TauriShellClient, type ShellClient } from "./shellClient";
import type { SessionDescriptor } from "../terminal/types";
import {
  VimInputStateMachine,
  type InputMode,
  type VimLineSnapshot,
} from "../vim";

const COMPLETION_DELAY_MILLISECONDS = 65;
const MAX_VISIBLE_COMPLETIONS = 6;
const MAX_PENDING_MARKERS = 12;

export type ShellExperiencePhase = "unavailable" | "waiting" | "editing";

export interface ShellExperienceStatus {
  readonly phase: ShellExperiencePhase;
  readonly cwd: string | null;
  readonly suggestionAvailable: boolean;
  readonly completionCount: number;
  readonly tokenKind: string | null;
  readonly inputMode: InputMode | null;
}

export interface ShellExperienceControllerOptions {
  readonly terminal: Terminal;
  readonly host: HTMLElement;
  readonly sendData: (data: string) => void;
  readonly onCommandAccepted: (command: string) => void;
  readonly onStatusChange: (status: ShellExperienceStatus) => void;
  readonly client?: ShellClient;
  readonly history?: readonly HistoryEntry[];
  readonly active?: boolean;
  readonly vimMode?: boolean;
}

interface ConsumedKeySequence {
  readonly key: string;
  readonly code: string;
  readonly terminalData: string | null;
}

interface InlineSuggestion {
  readonly suffix: string;
}

export class ShellExperienceController {
  private readonly terminal: Terminal;
  private readonly host: HTMLElement;
  private readonly client: ShellClient;
  private readonly sendData: (data: string) => void;
  private readonly onCommandAccepted: (command: string) => void;
  private readonly onStatusChange: (status: ShellExperienceStatus) => void;
  private readonly disposables: IDisposable[] = [];
  private readonly historySource: HistorySource = {
    entries: () => this.history,
  };
  private readonly historyNavigator = new HistoryNavigator(this.historySource);
  private readonly vimInput = new VimInputStateMachine();
  private readonly overlay: HTMLDivElement;
  private readonly suggestionElement: HTMLDivElement;
  private readonly completionElement: HTMLDivElement;
  private session: SessionDescriptor | null = null;
  private model: PromptInputModel | null = null;
  private history: readonly HistoryEntry[];
  private currentCwd: string | null = null;
  private pendingMarkers: string[] = [];
  private suggestion: InlineSuggestion | null = null;
  private completions: readonly CompletionChoice[] = [];
  private selectedCompletion = 0;
  private completionMenuOpen = false;
  private openCompletionsWhenReady = false;
  private completeWithTabWhenReady = false;
  private completionTimer: ReturnType<typeof setTimeout> | null = null;
  private completionGeneration = 0;
  private active: boolean;
  private vimModeEnabled: boolean;
  private pendingNormalModeData: string | null = null;
  private normalModeDataGeneration = 0;
  private consumedKeySequence: ConsumedKeySequence | null = null;
  private disposed = false;
  private lastStatus = "";
  private readonly handleWindowBlur = (): void => {
    this.clearConsumedKeySequence();
  };

  constructor(options: ShellExperienceControllerOptions) {
    this.terminal = options.terminal;
    this.host = options.host;
    this.client = options.client ?? new TauriShellClient();
    this.sendData = options.sendData;
    this.onCommandAccepted = options.onCommandAccepted;
    this.onStatusChange = options.onStatusChange;
    this.history = options.history ?? [];
    this.active = options.active ?? true;
    this.vimModeEnabled = options.vimMode ?? false;

    this.overlay = document.createElement("div");
    this.overlay.className = "shell-experience-overlay";
    this.suggestionElement = document.createElement("div");
    this.suggestionElement.className = "terminal-autosuggestion";
    this.suggestionElement.setAttribute("aria-hidden", "true");
    this.completionElement = document.createElement("div");
    this.completionElement.className = "terminal-completions";
    this.completionElement.setAttribute("role", "listbox");
    this.completionElement.setAttribute("aria-label", "Command completions");
    this.completionElement.hidden = true;
    this.overlay.append(this.suggestionElement, this.completionElement);
    this.host.append(this.overlay);
    window.addEventListener("blur", this.handleWindowBlur);

    this.disposables.push(
      this.terminal.parser.registerOscHandler(133, (data) =>
        this.handleOsc133(data),
      ),
      this.terminal.onRender(() => this.positionOverlay()),
      this.terminal.onScroll(() => this.refresh()),
      this.terminal.onResize(() => this.positionOverlay()),
    );
    this.terminal.attachCustomKeyEventHandler((event) =>
      this.handleKeyEvent(event),
    );
    this.emitStatus();
  }

  setSession(session: SessionDescriptor): void {
    this.session = session;
    if (session.shellIntegration && session.shellIntegrationNonce) {
      this.currentCwd = session.cwd;
      this.model = new PromptInputModel(session.shellIntegrationNonce);
      for (const marker of this.pendingMarkers.splice(0)) {
        this.applyOsc133(marker);
      }
    } else {
      this.currentCwd = null;
      this.pendingMarkers = [];
      this.model = null;
      this.vimInput.endPrompt();
    }
    this.refresh();
  }

  updateHistory(history: readonly HistoryEntry[]): void {
    this.history = history;
    this.historyNavigator.reset();
    this.refresh();
  }

  setActive(active: boolean): void {
    const activeChanged = this.active !== active;
    this.active = active;
    if (activeChanged) {
      this.clearNormalModeDataAllowance();
      this.clearConsumedKeySequence();
    }
    this.refresh();
  }

  setVimMode(enabled: boolean): void {
    if (this.vimModeEnabled === enabled) {
      return;
    }
    this.vimModeEnabled = enabled;
    this.clearNormalModeDataAllowance();
    this.clearConsumedKeySequence();
    this.closeCompletionMenu();
    const snapshot = this.model?.snapshot();
    if (enabled && snapshot?.phase === "editing" && snapshot.reliable) {
      this.vimInput.beginPrompt(snapshot);
    } else {
      this.vimInput.endPrompt();
    }
    this.refresh();
  }

  /**
   * Updates prompt state and decides whether xterm data may cross the PTY
   * boundary. TerminalController is the single owner of this routing decision,
   * which prevents paste and IME commits from bypassing Normal mode.
   */
  handleTerminalData(data: string): boolean {
    if (this.consumedKeySequence?.terminalData === data) {
      return false;
    }

    if (
      this.vimModeEnabled &&
      this.vimInput.mode === "normal" &&
      this.isPromptInputActive() &&
      !this.takeNormalModeDataAllowance(data)
    ) {
      return false;
    }

    const update = this.model?.handleData(data);
    if (!update?.changed) {
      return true;
    }
    const snapshot = this.model?.snapshot();
    if (
      this.vimModeEnabled &&
      snapshot?.phase === "editing" &&
      snapshot.reliable
    ) {
      this.vimInput.observe(snapshot);
    } else {
      this.vimInput.endPrompt();
    }
    this.historyNavigator.reset();
    this.closeCompletionMenu();
    this.refresh();
    return true;
  }

  dispose(): void {
    if (this.disposed) {
      return;
    }
    this.disposed = true;
    this.clearNormalModeDataAllowance();
    this.clearConsumedKeySequence();
    this.cancelCompletionQuery();
    window.removeEventListener("blur", this.handleWindowBlur);
    for (const disposable of this.disposables.splice(0)) {
      disposable.dispose();
    }
    this.overlay.remove();
  }

  private handleOsc133(data: string): boolean {
    if (!this.model) {
      if (this.pendingMarkers.length === MAX_PENDING_MARKERS) {
        this.pendingMarkers.shift();
      }
      this.pendingMarkers.push(data);
      return true;
    }
    this.applyOsc133(data);
    return true;
  }

  private applyOsc133(data: string): void {
    const nonce = this.session?.shellIntegrationNonce;
    const cwd = nonce ? parseCwdProperty(data, nonce) : null;
    const cwdChanged = cwd !== null && cwd !== this.currentCwd;
    if (cwd !== null) {
      this.currentCwd = cwd;
    }
    const previousPhase = this.model?.snapshot().phase;
    const update = this.model?.handleOsc133(data);
    if (!update?.changed) {
      if (cwdChanged) {
        this.refresh();
      }
      return;
    }
    const snapshot = this.model?.snapshot();
    if (this.vimModeEnabled && snapshot?.phase === "editing") {
      if (previousPhase !== "editing") {
        this.vimInput.beginPrompt(snapshot);
      } else {
        this.vimInput.observe(snapshot);
      }
    } else {
      this.vimInput.endPrompt();
    }
    if (update.committedCommand) {
      this.onCommandAccepted(update.committedCommand);
    }
    this.historyNavigator.reset();
    this.closeCompletionMenu();
    this.refresh();
  }

  private handleKeyEvent(event: KeyboardEvent): boolean {
    if (event.type !== "keydown") {
      if (!this.matchesConsumedKeySequence(event)) {
        return true;
      }
      if (event.type === "keyup") {
        this.clearConsumedKeySequence();
        return true;
      }
      event.preventDefault();
      return false;
    }

    if (this.consumedKeySequence) {
      this.clearConsumedKeySequence();
    }

    const shouldProcess = this.handleKeyDown(event);
    if (!shouldProcess) {
      this.consumedKeySequence = {
        key: event.key,
        code: event.code,
        terminalData:
          !event.ctrlKey &&
          !event.altKey &&
          !event.metaKey &&
          event.key.length === 1
            ? event.key
            : null,
      };
      event.preventDefault();
    }
    return shouldProcess;
  }

  private handleKeyDown(event: KeyboardEvent): boolean {
    if (event.isComposing || !this.isPromptInputActive()) {
      return true;
    }

    if (this.completionMenuOpen) {
      if (event.key === "Escape") {
        this.closeCompletionMenu();
        this.render();
        return false;
      }
      if (event.key === "ArrowDown" || event.key === "ArrowUp") {
        const direction = event.key === "ArrowDown" ? 1 : -1;
        this.selectCompletion(direction);
        return false;
      }
      if (
        event.key === "Enter" ||
        event.key === "ArrowRight" ||
        (event.key === "Tab" && !event.ctrlKey && !event.metaKey && !event.altKey)
      ) {
        this.acceptSelectedCompletion();
        return false;
      }
    }

    const snapshot = this.model?.snapshot();
    if (!snapshot) {
      return true;
    }

    if (this.vimModeEnabled) {
      const result = this.vimInput.handleKey(event, snapshot);
      if (result.kind === "handled") {
        this.revealPrompt();
        this.applyVimEdit(snapshot, result.edit);
        return false;
      }
      if (result.kind === "history") {
        this.revealPrompt();
        this.navigateHistory(result.direction, snapshot, true);
        return false;
      }
      if (this.vimInput.mode !== "insert") {
        this.allowNormalModePtyControl(event);
        return true;
      }
    }

    if (snapshot.cursor !== snapshot.line.length) {
      return true;
    }

    if (
      event.code === "Space" &&
      event.ctrlKey &&
      !event.metaKey &&
      !event.altKey
    ) {
      if (this.completions.length > 0) {
        this.openCompletionMenu();
      } else {
        this.openCompletionsWhenReady = true;
        this.refreshCompletions(true);
      }
      return false;
    }

    if (
      event.key === "Tab" &&
      !event.ctrlKey &&
      !event.metaKey &&
      !event.altKey
    ) {
      if (this.completions.length === 1) {
        this.acceptCompletion(this.completions[0]);
        return false;
      }
      if (this.completions.length > 1) {
        this.openCompletionMenu();
        return false;
      }
      if (
        !completionRequestForLine(
          snapshot.line,
          snapshot.cursor,
          this.currentCwd,
        )
      ) {
        return true;
      }
      this.completeWithTabWhenReady = true;
      this.refreshCompletions(true);
      return false;
    }

    if (
      (event.key === "ArrowRight" ||
        (event.key.toLowerCase() === "f" && event.ctrlKey)) &&
      !event.metaKey &&
      !event.altKey &&
      this.suggestion
    ) {
      this.acceptSuggestion();
      return false;
    }

    if (
      (event.key === "ArrowUp" || event.key === "ArrowDown") &&
      !event.ctrlKey &&
      !event.metaKey &&
      !event.altKey
    ) {
      return this.navigateHistory(
        event.key === "ArrowUp" ? "previous" : "next",
        snapshot,
        false,
      );
    }

    return true;
  }

  private matchesConsumedKeySequence(event: KeyboardEvent): boolean {
    const consumed = this.consumedKeySequence;
    if (!consumed) {
      return false;
    }
    if (consumed.code && event.code) {
      return consumed.code === event.code;
    }
    return consumed.key === event.key;
  }

  private clearConsumedKeySequence(): void {
    this.consumedKeySequence = null;
  }

  private navigateHistory(
    direction: "previous" | "next",
    current: VimLineSnapshot,
    consumeWhenUnchanged: boolean,
  ): boolean {
    const replacement =
      direction === "previous"
        ? this.historyNavigator.previous(current.line)
        : this.historyNavigator.next(current.line);
    if (replacement === current.line) {
      return consumeWhenUnchanged ? false : !this.historyNavigator.isNavigating;
    }

    let target: VimLineSnapshot = {
      line: replacement,
      cursor: replacement.length,
    };
    if (this.vimModeEnabled) {
      target = this.vimInput.observe(target);
    }
    this.synchronizeShellLine(current, target);
    this.closeCompletionMenu();
    this.refresh();
    return false;
  }

  private applyVimEdit(
    current: VimLineSnapshot,
    target: VimLineSnapshot,
  ): void {
    this.synchronizeShellLine(current, target);
    if (current.line !== target.line) {
      this.historyNavigator.reset();
    }
    this.closeCompletionMenu();
    this.refresh();
  }

  private synchronizeShellLine(
    current: VimLineSnapshot,
    target: VimLineSnapshot,
  ): void {
    if (current.line === target.line) {
      this.moveShellCursor(current.line, current.cursor, target.cursor);
      this.model?.replaceLine(target.line, target.cursor);
      return;
    }

    this.sendRepeated(
      "\u001b[C",
      this.countShellEditUnits(current.line.slice(current.cursor)),
    );
    this.sendRepeated("\u007f", this.countShellEditUnits(current.line));
    if (target.line) {
      this.sendData(target.line);
    }
    this.sendRepeated(
      "\u001b[D",
      this.countShellEditUnits(target.line.slice(target.cursor)),
    );
    this.model?.replaceLine(target.line, target.cursor);
  }

  private moveShellCursor(line: string, from: number, to: number): void {
    if (from === to) {
      return;
    }
    if (to < from) {
      this.sendRepeated(
        "\u001b[D",
        this.countShellEditUnits(line.slice(to, from)),
      );
    } else {
      this.sendRepeated(
        "\u001b[C",
        this.countShellEditUnits(line.slice(from, to)),
      );
    }
  }

  private countShellEditUnits(value: string): number {
    const shellName = this.session?.shellName.toLowerCase() ?? "";
    // PSReadLine indexes its StringBuilder in UTF-16 code units. Readline and
    // ZLE use decoded Unicode scalar values. Both representations can address
    // every grapheme boundary selected by the Vim state machine.
    return shellName === "pwsh" || shellName.includes("powershell")
      ? value.length
      : Array.from(value).length;
  }

  private sendRepeated(sequence: string, count: number): void {
    const repetitionsPerChunk = Math.max(
      1,
      Math.floor(16_384 / sequence.length),
    );
    for (
      let remaining = count;
      remaining > 0;
      remaining -= repetitionsPerChunk
    ) {
      this.sendData(sequence.repeat(Math.min(remaining, repetitionsPerChunk)));
    }
  }

  private revealPrompt(): void {
    const buffer = this.terminal.buffer.active;
    if (buffer.viewportY !== buffer.baseY) {
      this.terminal.scrollToBottom();
    }
  }

  private allowNormalModePtyControl(event: KeyboardEvent): void {
    const data = normalModePtyControlForEvent(event);
    this.pendingNormalModeData = data;
    const generation = ++this.normalModeDataGeneration;
    queueMicrotask(() => {
      if (this.normalModeDataGeneration === generation) {
        this.pendingNormalModeData = null;
      }
    });
  }

  private takeNormalModeDataAllowance(data: string): boolean {
    const allowed = this.pendingNormalModeData === data;
    this.clearNormalModeDataAllowance();
    return allowed;
  }

  private clearNormalModeDataAllowance(): void {
    this.normalModeDataGeneration += 1;
    this.pendingNormalModeData = null;
  }

  private acceptSuggestion(): void {
    const suggestion = this.suggestion;
    if (!suggestion) {
      return;
    }
    this.sendData(suggestion.suffix);
    this.model?.insertSynthetic(suggestion.suffix);
    this.observeVimModel();
    this.historyNavigator.reset();
    this.refresh();
  }

  private acceptSelectedCompletion(): void {
    this.acceptCompletion(this.completions[this.selectedCompletion]);
  }

  private acceptCompletion(completion: CompletionChoice | undefined): void {
    if (!completion) {
      return;
    }
    this.sendData(completion.insertion);
    this.model?.insertSynthetic(completion.insertion);
    this.observeVimModel();
    this.historyNavigator.reset();
    this.closeCompletionMenu();
    this.refresh();
  }

  private selectCompletion(direction: number): void {
    if (this.completions.length === 0) {
      return;
    }
    this.selectedCompletion =
      (this.selectedCompletion + direction + this.completions.length) %
      this.completions.length;
    this.render();
  }

  private openCompletionMenu(): void {
    if (this.completions.length === 0) {
      return;
    }
    this.completionMenuOpen = true;
    this.openCompletionsWhenReady = false;
    this.completeWithTabWhenReady = false;
    this.selectedCompletion = Math.min(
      this.selectedCompletion,
      this.completions.length - 1,
    );
    this.render();
  }

  private closeCompletionMenu(): void {
    this.completionMenuOpen = false;
    this.openCompletionsWhenReady = false;
    this.completeWithTabWhenReady = false;
    this.selectedCompletion = 0;
  }

  private refresh(): void {
    if (!this.isAssistanceActive()) {
      this.suggestion = null;
      this.completions = [];
      this.closeCompletionMenu();
      this.cancelCompletionQuery();
      this.render();
      return;
    }

    const snapshot = this.model?.snapshot();
    this.suggestion =
      snapshot && snapshot.cursor === snapshot.line.length
        ? bestAutosuggestion(snapshot.line, this.history)
        : null;
    this.refreshCompletions();
    this.render();
  }

  private refreshCompletions(immediate = false): void {
    const snapshot = this.model?.snapshot();
    const session = this.session;
    if (!snapshot || !session || !this.isAssistanceActive()) {
      this.completions = [];
      this.cancelCompletionQuery();
      this.render();
      return;
    }

    const request = completionRequestForLine(
      snapshot.line,
      snapshot.cursor,
      this.currentCwd,
    );
    this.cancelCompletionQuery();
    this.completions = [];
    if (!request) {
      this.closeCompletionMenu();
      this.render();
      return;
    }

    const generation = ++this.completionGeneration;
    const run = () => {
      this.completionTimer = null;
      void this.client
        .complete(session.sessionId, request)
        .then((candidates) => {
          if (this.disposed || generation !== this.completionGeneration) {
            return;
          }
          const current = this.model?.snapshot();
          if (!current || current.line !== snapshot.line || current.cursor !== snapshot.cursor) {
            return;
          }
          this.completions = buildCompletionChoices(
            current.line,
            current.cursor,
            candidates,
          ).slice(0, MAX_VISIBLE_COMPLETIONS);
          if (!this.suggestion) {
            const completion = this.completions.find(
              ({ kind }) => kind !== "environment",
            );
            this.suggestion = completion
              ? { suffix: completion.insertion }
              : null;
          }
          if (this.completeWithTabWhenReady) {
            this.completeWithTabWhenReady = false;
            if (this.completions.length === 1) {
              this.acceptCompletion(this.completions[0]);
            } else if (this.completions.length > 1) {
              this.openCompletionMenu();
            } else {
              this.passTabToShell();
            }
          } else if (
            this.openCompletionsWhenReady &&
            this.completions.length > 0
          ) {
            this.openCompletionMenu();
          } else {
            this.render();
          }
        })
        .catch(() => {
          if (generation === this.completionGeneration) {
            this.completions = [];
            if (this.completeWithTabWhenReady) {
              this.completeWithTabWhenReady = false;
              this.passTabToShell();
            } else {
              this.closeCompletionMenu();
              this.render();
            }
          }
        });
    };
    if (immediate) {
      run();
    } else {
      this.completionTimer = setTimeout(run, COMPLETION_DELAY_MILLISECONDS);
    }
  }

  private cancelCompletionQuery(): void {
    this.completionGeneration += 1;
    if (this.completionTimer !== null) {
      clearTimeout(this.completionTimer);
      this.completionTimer = null;
    }
  }

  private passTabToShell(): void {
    this.openCompletionsWhenReady = false;
    this.completionMenuOpen = false;
    this.sendData("\t");
    this.model?.handleData("\t");
    this.observeVimModel();
    this.historyNavigator.reset();
    this.suggestion = null;
    this.completions = [];
    this.render();
  }

  private isPromptInputActive(): boolean {
    if (!this.active || !this.session || !this.model || this.disposed) {
      return false;
    }
    const snapshot = this.model.snapshot();
    const buffer = this.terminal.buffer.active;
    return (
      snapshot.phase === "editing" &&
      snapshot.reliable &&
      buffer.type === "normal"
    );
  }

  private isOverlaySafe(): boolean {
    const buffer = this.terminal.buffer.active;
    return this.isPromptInputActive() && buffer.viewportY === buffer.baseY;
  }

  private isAssistanceActive(): boolean {
    return (
      this.isOverlaySafe() &&
      (!this.vimModeEnabled || this.vimInput.mode === "insert")
    );
  }

  private observeVimModel(): void {
    const snapshot = this.model?.snapshot();
    if (
      this.vimModeEnabled &&
      snapshot?.phase === "editing" &&
      snapshot.reliable
    ) {
      this.vimInput.observe(snapshot);
    }
  }

  private render(): void {
    const safe = this.isAssistanceActive();
    this.suggestionElement.textContent = safe
      ? (this.suggestion?.suffix ?? "")
      : "";
    this.suggestionElement.hidden = !safe || !this.suggestion;

    this.completionElement.replaceChildren();
    this.completionElement.hidden =
      !safe || !this.completionMenuOpen || this.completions.length === 0;
    if (!this.completionElement.hidden) {
      this.completions
        .slice(0, MAX_VISIBLE_COMPLETIONS)
        .forEach((completion, index) => {
          const option = document.createElement("button");
          option.type = "button";
          option.tabIndex = -1;
          option.className = "terminal-completion-option";
          option.classList.toggle("is-selected", index === this.selectedCompletion);
          option.setAttribute("role", "option");
          option.setAttribute(
            "aria-selected",
            index === this.selectedCompletion ? "true" : "false",
          );
          option.dataset.kind = completion.kind;

          const label = document.createElement("span");
          label.className = "terminal-completion-label";
          label.textContent = completion.display;
          const kind = document.createElement("span");
          kind.className = "terminal-completion-kind";
          kind.textContent = completion.kind;
          option.append(label, kind);
          option.addEventListener("mousedown", (event) => event.preventDefault());
          option.addEventListener("click", () => {
            this.acceptCompletion(completion);
            this.terminal.focus();
          });
          this.completionElement.append(option);
        });
    }

    this.positionOverlay();
    this.emitStatus();
  }

  private positionOverlay(): void {
    if (this.overlay.hidden || !this.active) {
      return;
    }
    const screen = this.terminal.element?.querySelector<HTMLElement>(
      ".xterm-screen",
    );
    if (!screen || this.terminal.cols <= 0 || this.terminal.rows <= 0) {
      return;
    }
    const hostBounds = this.host.getBoundingClientRect();
    const screenBounds = screen.getBoundingClientRect();
    const cellWidth = screenBounds.width / this.terminal.cols;
    const cellHeight = screenBounds.height / this.terminal.rows;
    const buffer = this.terminal.buffer.active;
    const left =
      screenBounds.left - hostBounds.left + buffer.cursorX * cellWidth;
    const top = screenBounds.top - hostBounds.top + buffer.cursorY * cellHeight;
    this.suggestionElement.style.left = `${left}px`;
    this.suggestionElement.style.top = `${top}px`;

    const menuHeight =
      Math.min(this.completions.length, MAX_VISIBLE_COMPLETIONS) * 28 + 8;
    const below = top + cellHeight;
    const menuTop =
      below + menuHeight <= this.host.clientHeight
        ? below
        : Math.max(0, top - menuHeight);
    this.completionElement.style.left = `${Math.min(left, Math.max(0, this.host.clientWidth - 300))}px`;
    this.completionElement.style.top = `${menuTop}px`;
  }

  private emitStatus(): void {
    const snapshot = this.model?.snapshot();
    const status: ShellExperienceStatus = {
      phase: this.isPromptInputActive()
        ? "editing"
        : this.session?.shellIntegration
          ? "waiting"
          : "unavailable",
      cwd: this.model ? this.currentCwd : null,
      suggestionAvailable: Boolean(this.suggestion),
      completionCount: this.completions.length,
      tokenKind:
        snapshot?.phase === "editing"
          ? syntaxContext(snapshot.line, snapshot.cursor).kind
          : null,
      inputMode:
        this.vimModeEnabled && this.isPromptInputActive()
          ? this.vimInput.mode
          : null,
    };
    const serialized = JSON.stringify(status);
    if (serialized !== this.lastStatus) {
      this.lastStatus = serialized;
      this.onStatusChange(status);
    }
  }
}

export function parseCwdProperty(data: string, nonce: string): string | null {
  const parts = data.split(";");
  if (
    parts.length !== 3 ||
    parts[0] !== "P" ||
    !parts[1]?.startsWith("CwdHex=") ||
    parts[2] !== nonce
  ) {
    return null;
  }

  const hex = parts[1].slice("CwdHex=".length);
  if (!hex || hex.length > 8_192 || hex.length % 2 !== 0 || !/^[0-9a-f]+$/i.test(hex)) {
    return null;
  }
  try {
    const bytes = new Uint8Array(hex.length / 2);
    for (let index = 0; index < bytes.length; index += 1) {
      bytes[index] = Number.parseInt(hex.slice(index * 2, index * 2 + 2), 16);
    }
    const path = new TextDecoder("utf-8", { fatal: true }).decode(bytes);
    if (!path || Array.from(path).some(isControlCharacter)) {
      return null;
    }
    return path;
  } catch {
    return null;
  }
}

function isControlCharacter(character: string): boolean {
  const point = character.codePointAt(0) ?? 0;
  return point <= 0x1f || (point >= 0x7f && point <= 0x9f);
}

function normalModePtyControlForEvent(event: KeyboardEvent): string | null {
  if (event.altKey || event.metaKey) {
    return null;
  }
  if (event.key === "Enter") {
    return "\r";
  }
  if (!event.ctrlKey || event.key.length !== 1) {
    return null;
  }

  switch (event.key.toLowerCase()) {
    case "c":
      return "\u0003";
    case "d":
      return "\u0004";
    case "j":
      return "\n";
    case "l":
      return "\u000c";
    case "m":
      return "\r";
    case "q":
      return "\u0011";
    case "s":
      return "\u0013";
    case "z":
      return "\u001a";
    case "\\":
      return "\u001c";
    default:
      return null;
  }
}

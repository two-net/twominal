export type VimMode = "NORMAL" | "INSERT" | "VISUAL";

export interface VimKeyResult {
  handled: boolean;
  newBuffer: string;
  newCursor: number;
  action?: "submit" | "none";
}

export type VimModeChangeListener = (mode: VimMode, isEnabled: boolean) => void;

export class VimModeEngine {
  private mode: VimMode = "INSERT";
  private isEnabled: boolean = true;
  private clipboard: string = "";
  private pendingOperator: string = "";
  private undoStack: Array<{ text: string; cursor: number }> = [];
  private listeners: Set<VimModeChangeListener> = new Set();

  constructor() {
    this.loadSettings();
  }

  private loadSettings(): void {
    try {
      // Clean up poisoned legacy keys that stored 'false' in older builds
      localStorage.removeItem("twominal_vim_enabled");
      localStorage.removeItem("twominal_vim_initialized");

      const saved = localStorage.getItem("twominal_vim_mode");
      if (saved === "off") {
        this.isEnabled = false;
      } else {
        this.isEnabled = true;
        localStorage.setItem("twominal_vim_mode", "on");
      }
    } catch {
      this.isEnabled = true;
    }
  }

  public setEnabled(enabled: boolean, savePreference: boolean = false): void {
    this.isEnabled = enabled;
    if (!enabled) {
      this.mode = "INSERT";
    }
    if (savePreference) {
      try {
        localStorage.setItem("twominal_vim_mode", enabled ? "on" : "off");
      } catch {
        // Ignore
      }
    }
    this.notifyListeners();
  }

  public toggle(): boolean {
    this.setEnabled(!this.isEnabled, true);
    return this.isEnabled;
  }

  public getIsEnabled(): boolean {
    return this.isEnabled;
  }

  public getMode(): VimMode {
    return this.mode;
  }

  public setMode(mode: VimMode): void {
    if (!this.isEnabled) return;
    this.mode = mode;
    this.pendingOperator = "";
    this.notifyListeners();
  }

  public onModeChange(listener: VimModeChangeListener): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  private notifyListeners(): void {
    for (const listener of this.listeners) {
      try {
        listener(this.mode, this.isEnabled);
      } catch (err) {
        console.error("Vim listener error:", err);
      }
    }
  }

  public handleKey(
    key: string,
    ctrlKey: boolean,
    altKey: boolean,
    buffer: string,
    cursorPos: number
  ): VimKeyResult {
    if (!this.isEnabled) {
      return { handled: false, newBuffer: buffer, newCursor: cursorPos };
    }

    // Escape always returns to NORMAL mode
    if (key === "Escape") {
      this.setMode("NORMAL");
      const newCursor = Math.max(0, Math.min(cursorPos, Math.max(0, buffer.length - 1)));
      return { handled: true, newBuffer: buffer, newCursor };
    }

    if (this.mode === "INSERT") {
      return { handled: false, newBuffer: buffer, newCursor: cursorPos };
    }

    const pushUndo = (b: string, c: number) => {
      this.undoStack.push({ text: b, cursor: c });
      if (this.undoStack.length > 50) this.undoStack.shift();
    };

    if (this.mode === "VISUAL") {
      if (key === "y") {
        this.clipboard = buffer.slice(cursorPos, cursorPos + 1);
        this.setMode("NORMAL");
        return { handled: true, newBuffer: buffer, newCursor: cursorPos };
      }
      if (key === "d" || key === "x") {
        pushUndo(buffer, cursorPos);
        const newBuf = buffer.slice(0, cursorPos) + buffer.slice(cursorPos + 1);
        this.setMode("NORMAL");
        return { handled: true, newBuffer: newBuf, newCursor: Math.max(0, cursorPos - 1) };
      }
      if (key === "c") {
        pushUndo(buffer, cursorPos);
        const newBuf = buffer.slice(0, cursorPos) + buffer.slice(cursorPos + 1);
        this.setMode("INSERT");
        return { handled: true, newBuffer: newBuf, newCursor: cursorPos };
      }
      if (key === "h") {
        return { handled: true, newBuffer: buffer, newCursor: Math.max(0, cursorPos - 1) };
      }
      if (key === "l") {
        return { handled: true, newBuffer: buffer, newCursor: Math.min(buffer.length - 1, cursorPos + 1) };
      }
      return { handled: true, newBuffer: buffer, newCursor: cursorPos };
    }

    // NORMAL MODE
    if (this.mode === "NORMAL") {
      if (ctrlKey || altKey) {
        return { handled: false, newBuffer: buffer, newCursor: cursorPos };
      }

      if (this.pendingOperator) {
        const op = this.pendingOperator;
        this.pendingOperator = "";

        if (op === "d" && key === "d") {
          pushUndo(buffer, cursorPos);
          this.clipboard = buffer;
          return { handled: true, newBuffer: "", newCursor: 0 };
        }
        if (op === "d" && key === "w") {
          pushUndo(buffer, cursorPos);
          const wordEnd = this.findNextWordIndex(buffer, cursorPos);
          const deleted = buffer.slice(cursorPos, wordEnd);
          this.clipboard = deleted;
          const newBuf = buffer.slice(0, cursorPos) + buffer.slice(wordEnd);
          return { handled: true, newBuffer: newBuf, newCursor: Math.min(cursorPos, Math.max(0, newBuf.length - 1)) };
        }
        if (op === "c" && key === "w") {
          pushUndo(buffer, cursorPos);
          const wordEnd = this.findNextWordIndex(buffer, cursorPos);
          const newBuf = buffer.slice(0, cursorPos) + buffer.slice(wordEnd);
          this.setMode("INSERT");
          return { handled: true, newBuffer: newBuf, newCursor: cursorPos };
        }
        if (op === "y" && key === "y") {
          this.clipboard = buffer;
          return { handled: true, newBuffer: buffer, newCursor: cursorPos };
        }
        return { handled: true, newBuffer: buffer, newCursor: cursorPos };
      }

      switch (key) {
        case "i":
          this.setMode("INSERT");
          return { handled: true, newBuffer: buffer, newCursor: cursorPos };

        case "a":
          this.setMode("INSERT");
          return {
            handled: true,
            newBuffer: buffer,
            newCursor: Math.min(buffer.length, cursorPos + 1),
          };

        case "I":
          this.setMode("INSERT");
          return { handled: true, newBuffer: buffer, newCursor: 0 };

        case "A":
          this.setMode("INSERT");
          return { handled: true, newBuffer: buffer, newCursor: buffer.length };

        case "h":
          return {
            handled: true,
            newBuffer: buffer,
            newCursor: Math.max(0, cursorPos - 1),
          };

        case "l":
          return {
            handled: true,
            newBuffer: buffer,
            newCursor: Math.min(Math.max(0, buffer.length - 1), cursorPos + 1),
          };

        case "w": {
          const next = this.findNextWordIndex(buffer, cursorPos);
          return { handled: true, newBuffer: buffer, newCursor: next };
        }

        case "b": {
          const prev = this.findPrevWordIndex(buffer, cursorPos);
          return { handled: true, newBuffer: buffer, newCursor: prev };
        }

        case "0":
        case "^":
          return { handled: true, newBuffer: buffer, newCursor: 0 };

        case "$":
          return {
            handled: true,
            newBuffer: buffer,
            newCursor: Math.max(0, buffer.length - 1),
          };

        case "x": {
          if (buffer.length === 0) return { handled: true, newBuffer: buffer, newCursor: 0 };
          pushUndo(buffer, cursorPos);
          this.clipboard = buffer[cursorPos] || "";
          const newBuf = buffer.slice(0, cursorPos) + buffer.slice(cursorPos + 1);
          return {
            handled: true,
            newBuffer: newBuf,
            newCursor: Math.min(cursorPos, Math.max(0, newBuf.length - 1)),
          };
        }

        case "d":
        case "c":
        case "y":
          this.pendingOperator = key;
          return { handled: true, newBuffer: buffer, newCursor: cursorPos };

        case "D": {
          pushUndo(buffer, cursorPos);
          this.clipboard = buffer.slice(cursorPos);
          const newBuf = buffer.slice(0, cursorPos);
          return {
            handled: true,
            newBuffer: newBuf,
            newCursor: Math.max(0, cursorPos - 1),
          };
        }

        case "C": {
          pushUndo(buffer, cursorPos);
          const newBuf = buffer.slice(0, cursorPos);
          this.setMode("INSERT");
          return { handled: true, newBuffer: newBuf, newCursor: cursorPos };
        }

        case "p": {
          if (!this.clipboard) return { handled: true, newBuffer: buffer, newCursor: cursorPos };
          pushUndo(buffer, cursorPos);
          const insertIdx = Math.min(buffer.length, cursorPos + 1);
          const newBuf = buffer.slice(0, insertIdx) + this.clipboard + buffer.slice(insertIdx);
          return {
            handled: true,
            newBuffer: newBuf,
            newCursor: insertIdx + this.clipboard.length - 1,
          };
        }

        case "P": {
          if (!this.clipboard) return { handled: true, newBuffer: buffer, newCursor: cursorPos };
          pushUndo(buffer, cursorPos);
          const newBuf = buffer.slice(0, cursorPos) + this.clipboard + buffer.slice(cursorPos);
          return {
            handled: true,
            newBuffer: newBuf,
            newCursor: cursorPos + this.clipboard.length - 1,
          };
        }

        case "u": {
          const snapshot = this.undoStack.pop();
          if (snapshot) {
            return {
              handled: true,
              newBuffer: snapshot.text,
              newCursor: snapshot.cursor,
            };
          }
          return { handled: true, newBuffer: buffer, newCursor: cursorPos };
        }

        case "v":
          this.setMode("VISUAL");
          return { handled: true, newBuffer: buffer, newCursor: cursorPos };

        case "Enter":
          return { handled: true, newBuffer: buffer, newCursor: cursorPos, action: "submit" };

        default:
          return { handled: true, newBuffer: buffer, newCursor: cursorPos };
      }
    }

    return { handled: false, newBuffer: buffer, newCursor: cursorPos };
  }

  private findNextWordIndex(buffer: string, cursor: number): number {
    let i = cursor;
    const len = buffer.length;
    while (i < len && !/\s/.test(buffer[i])) {
      i++;
    }
    while (i < len && /\s/.test(buffer[i])) {
      i++;
    }
    return Math.min(len, i);
  }

  private findPrevWordIndex(buffer: string, cursor: number): number {
    let i = cursor - 1;
    while (i > 0 && /\s/.test(buffer[i])) {
      i--;
    }
    while (i > 0 && !/\s/.test(buffer[i - 1])) {
      i--;
    }
    return Math.max(0, i);
  }
}

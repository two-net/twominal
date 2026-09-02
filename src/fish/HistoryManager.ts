import { invoke } from "@tauri-apps/api/core";

export class HistoryManager {
  private static instance: HistoryManager;
  private history: string[] = [
    "echo 'Welcome to Twominal!'",
    "neofetch",
    "ls -la"
  ];
  private historyIndex: number = -1;
  private currentDraft: string = "";

  private constructor() {
    this.loadHistory();
  }

  public static getInstance(): HistoryManager {
    if (!HistoryManager.instance) {
      HistoryManager.instance = new HistoryManager();
    }
    return HistoryManager.instance;
  }

  public async loadHistory(): Promise<void> {
    try {
      const items = await invoke<string[]>("fish_get_history", { limit: 500 });
      if (Array.isArray(items) && items.length > 0) {
        this.history = items;
      }
    } catch (err) {
      console.warn("Failed to load history from backend:", err);
    }
  }

  public async add(command: string): Promise<void> {
    const trimmed = command.trim();
    if (!trimmed) return;

    if (this.history.length > 0 && this.history[this.history.length - 1] === trimmed) {
      return;
    }

    this.history.push(trimmed);
    this.resetNavigation();

    try {
      await invoke("fish_add_history", { command: trimmed });
    } catch (err) {
      console.warn("Failed to persist history:", err);
    }
  }

  public findSuggestion(prefix: string): string | null {
    const trimmed = prefix.trimStart();
    if (!trimmed) return null;

    for (let i = this.history.length - 1; i >= 0; i--) {
      const entry = this.history[i];
      if (entry.toLowerCase().startsWith(trimmed.toLowerCase()) && entry.length > trimmed.length) {
        return entry.slice(trimmed.length);
      }
    }

    return null;
  }

  public resetNavigation(): void {
    this.historyIndex = -1;
    this.currentDraft = "";
  }

  public navigateUp(currentInput: string): string | null {
    if (this.history.length === 0) return null;

    if (this.historyIndex === -1) {
      this.currentDraft = currentInput;
      this.historyIndex = this.history.length - 1;
    } else if (this.historyIndex > 0) {
      this.historyIndex--;
    } else {
      return this.history[0];
    }

    return this.history[this.historyIndex] ?? null;
  }

  public navigateDown(): string | null {
    if (this.historyIndex === -1) return null;

    if (this.historyIndex < this.history.length - 1) {
      this.historyIndex++;
      return this.history[this.historyIndex];
    } else {
      this.historyIndex = -1;
      return this.currentDraft;
    }
  }

  public getAll(): string[] {
    return [...this.history];
  }
}

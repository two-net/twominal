import { invoke } from "@tauri-apps/api/core";
import { HistoryManager } from "./HistoryManager";
import { SyntaxHighlighter } from "./SyntaxHighlighter";
import { CompletionItem } from "./CompletionMenu";

const KNOWN_COMMANDS = [
  "/help", "/theme", "/font", "/vim", "/matrix", "/settings", "/clear", "/neofetch",
  "/ligatures", "/stack", "/history", "/date", "/tabs", "/exit",
  "help", "clear", "ls", "cd", "pwd", "cat", "echo", "touch", "mkdir", "rm", 
  "tree", "neofetch", "twominalfetch", "theme", "font", "vim", "stack", "ligatures", "matrix", "date", 
  "history", "settings", "whoami", "uname", "top", "curl", "fish_config", "exit", "git",
  "cargo", "pnpm", "npm", "yarn", "bun", "rustc", "node", "python", "python3",
  "go", "docker", "make", "grep", "find", "cp", "mv", "chmod", "chown", "kill",
  "ps", "df", "du", "head", "tail", "less", "more", "which", "where", "env"
];

export interface ParsedBuffer {
  cmdWord: string;
  prefixBeforeTarget: string;
  targetWord: string;
  isSingleCommandWord: boolean;
}

export class Autosuggestions {
  private historyManager: HistoryManager;
  // Cache directory listings: key is `${cwd}::dir::${dirPart}`
  private dirCompletionCache: Map<string, CompletionItem[]> = new Map();
  // Cache exact prefix queries: key is `${cwd}::${prefix}`
  private prefixCompletionCache: Map<string, CompletionItem[]> = new Map();
  private pendingFetches: Map<string, Promise<CompletionItem[]>> = new Map();
  private lastFullSuggestion: string = "";
  private listeners: Set<() => void> = new Set();

  constructor() {
    this.historyManager = HistoryManager.getInstance();
  }

  public onUpdate(listener: () => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  private notifyUpdate(): void {
    for (const listener of this.listeners) {
      try {
        listener();
      } catch (err) {
        console.error("Autosuggestions listener error:", err);
      }
    }
  }

  public clearCache(): void {
    this.dirCompletionCache.clear();
    this.prefixCompletionCache.clear();
    this.pendingFetches.clear();
  }

  public getLastFullSuggestion(): string {
    return this.lastFullSuggestion;
  }

  public parseBuffer(buffer: string): ParsedBuffer | null {
    if (!buffer) return null;

    const tokens = SyntaxHighlighter.tokenize(buffer);
    if (tokens.length === 0) return null;

    const firstCmdToken = tokens.find(t => t.type !== "whitespace" && t.type !== "comment");
    const cmdWord = firstCmdToken ? firstCmdToken.text : "";

    const lastToken = tokens[tokens.length - 1];

    if (lastToken.type === "whitespace") {
      return {
        cmdWord,
        prefixBeforeTarget: buffer,
        targetWord: "",
        isSingleCommandWord: false,
      };
    }

    let targetWord = lastToken.text;
    if (lastToken.type === "string" && (targetWord.startsWith('"') || targetWord.startsWith("'"))) {
      targetWord = targetWord.slice(1);
    }

    const prefixBeforeTarget = buffer.slice(0, lastToken.start);
    const isSingleCommandWord = firstCmdToken === lastToken;

    return {
      cmdWord,
      prefixBeforeTarget,
      targetWord,
      isSingleCommandWord,
    };
  }

  public getSuggestion(buffer: string, cwd?: string): string | null {
    if (!buffer || buffer.trim().length === 0) {
      this.lastFullSuggestion = "";
      return null;
    }

    const trimmed = buffer.trimStart();

    // 1. Check history first (highest priority in fish shell)
    const histSuggestion = this.historyManager.findSuggestion(trimmed);
    if (histSuggestion) {
      const full = buffer + histSuggestion;
      this.lastFullSuggestion = full;
      return full;
    }

    // 2. Parse command and target word being typed
    const parsed = this.parseBuffer(buffer);
    if (!parsed) return null;

    const { cmdWord, prefixBeforeTarget, targetWord, isSingleCommandWord } = parsed;

    // 3. Check known commands if single word (and not a path like ./ or / or ~/)
    if (
      isSingleCommandWord &&
      !targetWord.startsWith("./") &&
      !targetWord.startsWith("../") &&
      !targetWord.startsWith("/") &&
      !targetWord.startsWith("~")
    ) {
      const lower = targetWord.toLowerCase();
      for (const cmd of KNOWN_COMMANDS) {
        if (cmd.toLowerCase().startsWith(lower) && cmd.length > lower.length) {
          const full = prefixBeforeTarget + cmd;
          this.lastFullSuggestion = full;
          return full;
        }
      }
    }

    // 4. Check cached path & file completions
    const currentCwd = cwd || ".";
    const cachedItems = this.getCachedCompletions(currentCwd, targetWord);
    if (cachedItems && cachedItems.length > 0) {
      const match = this.pickBestMatch(cmdWord, targetWord, cachedItems);
      if (match) {
        const full = prefixBeforeTarget + match.value;
        this.lastFullSuggestion = full;
        return full;
      }
    }

    // 5. Trigger async fetch for path completions in background
    this.fetchCompletionsAsync(currentCwd, targetWord);

    return null;
  }

  public async getSuggestionAsync(buffer: string, cwd?: string): Promise<string | null> {
    const syncResult = this.getSuggestion(buffer, cwd);
    if (syncResult) {
      return syncResult;
    }

    if (!buffer || buffer.trim().length === 0) {
      return null;
    }

    const parsed = this.parseBuffer(buffer);
    if (!parsed) return null;

    const { cmdWord, prefixBeforeTarget, targetWord } = parsed;
    const currentCwd = cwd || ".";

    try {
      const items = await this.fetchCompletionsAsync(currentCwd, targetWord);
      if (items && items.length > 0) {
        const match = this.pickBestMatch(cmdWord, targetWord, items);
        if (match) {
          const full = prefixBeforeTarget + match.value;
          this.lastFullSuggestion = full;
          return full;
        }
      }
    } catch {
      // Ignore background errors
    }

    return null;
  }

  private pickBestMatch(cmdWord: string, targetWord: string, items: CompletionItem[]): CompletionItem | null {
    if (!items || items.length === 0) return null;

    const lowerTarget = targetWord.toLowerCase();
    
    // Filter matching items whose value starts with targetWord case-insensitively
    const matches = items.filter(item => {
      const val = item.value.toLowerCase();
      return val.startsWith(lowerTarget) && val.length > lowerTarget.length;
    });

    if (matches.length === 0) return null;

    // If command is `cd`, prioritize directories
    if (cmdWord.toLowerCase() === "cd") {
      const dirMatch = matches.find(m => m.kind === "dir" || m.value.endsWith("/"));
      if (dirMatch) return dirMatch;
    }

    return matches[0];
  }

  private getCachedCompletions(cwd: string, targetWord: string): CompletionItem[] | null {
    const prefixKey = `${cwd}::${targetWord}`;
    if (this.prefixCompletionCache.has(prefixKey)) {
      return this.prefixCompletionCache.get(prefixKey)!;
    }

    const lastSlash = Math.max(targetWord.lastIndexOf("/"), targetWord.lastIndexOf("\\"));
    const dirPart = lastSlash === -1 ? "" : targetWord.slice(0, lastSlash + 1);
    const dirKey = `${cwd}::dir::${dirPart}`;

    if (this.dirCompletionCache.has(dirKey)) {
      const dirItems = this.dirCompletionCache.get(dirKey)!;
      const fullTarget = targetWord.toLowerCase();
      return dirItems.filter(item => item.value.toLowerCase().startsWith(fullTarget));
    }

    return null;
  }

  private async fetchCompletionsAsync(cwd: string, targetWord: string): Promise<CompletionItem[]> {
    const prefixKey = `${cwd}::${targetWord}`;
    if (this.pendingFetches.has(prefixKey)) {
      return this.pendingFetches.get(prefixKey)!;
    }

    const lastSlash = Math.max(targetWord.lastIndexOf("/"), targetWord.lastIndexOf("\\"));
    const dirPart = lastSlash === -1 ? "" : targetWord.slice(0, lastSlash + 1);
    const dirKey = `${cwd}::dir::${dirPart}`;

    const fetchPromise = (async () => {
      try {
        const results = await invoke<CompletionItem[]>("fish_get_completions", {
          cwd: cwd || ".",
          prefix: targetWord,
        });

        const items = results || [];
        this.prefixCompletionCache.set(prefixKey, items);

        // Populate directory cache if querying a directory
        if (targetWord === dirPart) {
          this.dirCompletionCache.set(dirKey, items);
        }

        this.notifyUpdate();
        return items;
      } catch (err) {
        return [];
      } finally {
        this.pendingFetches.delete(prefixKey);
      }
    })();

    this.pendingFetches.set(prefixKey, fetchPromise);
    return fetchPromise;
  }

  public applyCompletion(currentInput: string, item: CompletionItem): string {
    const parsed = this.parseBuffer(currentInput);
    const prefix = parsed ? parsed.prefixBeforeTarget : "";
    const isDir = item.kind === "dir" || item.value.endsWith("/");
    return prefix + item.value + (isDir ? "" : " ");
  }

  public acceptNextWord(currentBuffer: string, fullSuggestion?: string): string {
    const suggestion = fullSuggestion || this.lastFullSuggestion;
    if (!suggestion) return currentBuffer;

    if (!suggestion.toLowerCase().startsWith(currentBuffer.toLowerCase())) {
      return currentBuffer;
    }

    const remaining = suggestion.slice(currentBuffer.length);
    if (!remaining) return currentBuffer;

    // Match leading whitespace + token up to next / or whitespace or end
    const match = remaining.match(/^(\s*[^/\s]+[/]?|\s+)/);
    if (match && match[1]) {
      const nextLen = currentBuffer.length + match[1].length;
      return suggestion.slice(0, nextLen);
    }

    return suggestion;
  }

  public acceptFull(currentBuffer: string, fullSuggestion?: string): string {
    const suggestion = fullSuggestion || this.lastFullSuggestion;
    if (!suggestion) return currentBuffer;

    if (!suggestion.toLowerCase().startsWith(currentBuffer.toLowerCase())) {
      return currentBuffer;
    }

    return suggestion;
  }
}

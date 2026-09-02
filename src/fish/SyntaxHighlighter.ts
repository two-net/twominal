import { invoke } from "@tauri-apps/api/core";

export type TokenType =
  | "command-valid"
  | "command-invalid"
  | "command-checking"
  | "flag"
  | "string"
  | "variable"
  | "path"
  | "operator"
  | "argument"
  | "comment"
  | "whitespace";

export interface SyntaxToken {
  type: TokenType;
  text: string;
  start: number;
  end: number;
}

interface CommandCheckResult {
  name: string;
  exists: boolean;
  kind: string;
}

export const BUILTIN_SLASH_COMMANDS = new Set([
  "/help", "/clear", "/theme", "/font", "/vim", "/matrix", "/stack", "/ligatures",
  "/neofetch", "/twominalfetch", "/history", "/date", "/settings", "/config",
  "/tabs", "/tab", "/exit", "/"
]);

const KNOWN_COMMANDS = new Set([
  "/help", "/clear", "/theme", "/font", "/vim", "/matrix", "/stack", "/ligatures",
  "/neofetch", "/twominalfetch", "/history", "/date", "/settings", "/config",
  "/tabs", "/tab", "/exit", "/",
  "help", "clear", "ls", "cd", "pwd", "cat", "echo", "touch", "mkdir", "rm",
  "tree", "neofetch", "twominalfetch", "theme", "font", "vim", "stack", "ligatures", "matrix", "date",
  "history", "settings", "config", "tabs", "tab", "whoami", "uname", "top", "curl", "fish_config", "exit", "git",
  "cargo", "pnpm", "npm", "yarn", "bun", "rustc", "node", "python", "python3",
  "go", "docker", "make", "grep", "find", "cp", "mv", "chmod", "chown", "kill",
  "ps", "df", "du", "head", "tail", "less", "more", "which", "where", "env"
]);

export class SyntaxHighlighter {
  private static commandCache: Map<string, boolean> = new Map();
  private static pendingChecks: Set<string> = new Set();
  private static listeners: Set<() => void> = new Set();

  static {
    for (const cmd of KNOWN_COMMANDS) {
      this.commandCache.set(cmd, true);
    }
  }

  public static onUpdate(listener: () => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  private static notifyUpdate(): void {
    for (const listener of this.listeners) {
      try {
        listener();
      } catch (err) {
        console.error("SyntaxHighlighter listener error:", err);
      }
    }
  }

  public static async checkCommandAsync(command: string): Promise<boolean> {
    const trimmed = command.trim();
    if (!trimmed) return false;

    if (this.commandCache.has(trimmed)) {
      return this.commandCache.get(trimmed)!;
    }

    if (this.pendingChecks.has(trimmed)) {
      return false;
    }

    this.pendingChecks.add(trimmed);
    try {
      const res = await invoke<CommandCheckResult>("fish_check_command", { command: trimmed });
      this.commandCache.set(trimmed, res.exists);
      this.pendingChecks.delete(trimmed);
      this.notifyUpdate();
      return res.exists;
    } catch {
      this.commandCache.set(trimmed, false);
      this.pendingChecks.delete(trimmed);
      return false;
    }
  }

  public static tokenize(input: string): SyntaxToken[] {
    const tokens: SyntaxToken[] = [];
    if (!input) return tokens;

    let i = 0;
    const len = input.length;
    let isExpectingCommand = true;

    const OPERATORS = [
      "<!--", "-->", "===", "!==", "<=>", "<==", "==>", "-->",
      "->", "=>", "<-", "<=", ">=", "==", "!=", "&&", "||",
      "??", "::", "...", "..", ">>", "<<", "|>", "<|",
      "++", "+=", "-=", "*=", "/=", "%=", "~>", "<~", "~~",
      "=~", "!~", ";;", ";", "|", "&", ">", "<", "="
    ];

    while (i < len) {
      const char = input[i];

      // 1. Whitespace
      if (/\s/.test(char)) {
        const start = i;
        while (i < len && /\s/.test(input[i])) {
          i++;
        }
        tokens.push({
          type: "whitespace",
          text: input.slice(start, i),
          start,
          end: i,
        });
        continue;
      }

      // 2. Comments (# ...)
      if (char === "#" && (i === 0 || /\s/.test(input[i - 1]))) {
        const start = i;
        tokens.push({
          type: "comment",
          text: input.slice(start),
          start,
          end: len,
        });
        break;
      }

      // 3. Strings ("..." or '...')
      if (char === '"' || char === "'") {
        const quote = char;
        const start = i;
        i++;
        while (i < len && input[i] !== quote) {
          if (input[i] === "\\" && i + 1 < len) {
            i += 2;
          } else {
            i++;
          }
        }
        if (i < len && input[i] === quote) {
          i++;
        }
        tokens.push({
          type: "string",
          text: input.slice(start, i),
          start,
          end: i,
        });
        isExpectingCommand = false;
        continue;
      }

      // 4. Variables ($VAR or ${VAR})
      if (char === "$") {
        const start = i;
        i++;
        if (i < len && input[i] === "{") {
          while (i < len && input[i] !== "}") {
            i++;
          }
          if (i < len && input[i] === "}") {
            i++;
          }
        } else {
          while (i < len && /[a-zA-Z0-9_]/.test(input[i])) {
            i++;
          }
        }
        tokens.push({
          type: "variable",
          text: input.slice(start, i),
          start,
          end: i,
        });
        isExpectingCommand = false;
        continue;
      }

      // 5. Flags (--flag, -f) - check before single '-' operator
      if (
        char === "-" &&
        !isExpectingCommand &&
        i + 1 < len &&
        /[a-zA-Z0-9]/.test(input[i + 1])
      ) {
        const start = i;
        while (i < len && !/[\s;|>&<="'#$]/.test(input[i])) {
          i++;
        }
        tokens.push({
          type: "flag",
          text: input.slice(start, i),
          start,
          end: i,
        });
        continue;
      }

      // 6. Unified Multi-character Operator & Ligature Tokenizer
      let matchedOp: string | null = null;
      for (const op of OPERATORS) {
        if (input.startsWith(op, i)) {
          matchedOp = op;
          break;
        }
      }

      if (matchedOp !== null) {
        const start = i;
        i += matchedOp.length;
        tokens.push({
          type: "operator",
          text: matchedOp,
          start,
          end: i,
        });
        if (
          matchedOp === ";" ||
          matchedOp === ";;" ||
          matchedOp === "&&" ||
          matchedOp === "||" ||
          matchedOp === "|"
        ) {
          isExpectingCommand = true;
        }
        continue;
      }

      // 7. Command, Path or Argument word
      const start = i;
      while (i < len && !/[\s;|>&<="'#$]/.test(input[i])) {
        // Stop if next characters match an operator
        if (OPERATORS.some((op) => input.startsWith(op, i))) {
          break;
        }
        i++;
      }
      const word = input.slice(start, i);
      if (!word) {
        // Fallback for single special character
        i++;
        tokens.push({
          type: "argument",
          text: char,
          start,
          end: i,
        });
        continue;
      }

      if (isExpectingCommand) {
        if (word.startsWith("./") || word.startsWith("/") || word.startsWith("~/")) {
          tokens.push({
            type: "command-valid",
            text: word,
            start,
            end: i,
          });
        } else if (this.commandCache.has(word)) {
          const isValid = this.commandCache.get(word)!;
          tokens.push({
            type: isValid ? "command-valid" : "command-invalid",
            text: word,
            start,
            end: i,
          });
        } else {
          tokens.push({
            type: "command-checking",
            text: word,
            start,
            end: i,
          });
          this.checkCommandAsync(word);
        }
        isExpectingCommand = false;
      } else {
        if (word.includes("/") || word.startsWith("~")) {
          tokens.push({
            type: "path",
            text: word,
            start,
            end: i,
          });
        } else {
          tokens.push({
            type: "argument",
            text: word,
            start,
            end: i,
          });
        }
      }
    }

    return tokens;
  }

  public static toHtml(tokens: SyntaxToken[]): string {
    return tokens
      .map((token) => {
        const escaped = escapeHtml(token.text);
        switch (token.type) {
          case "command-valid":
            return `<span class="text-twominal-fish-valid font-bold">${escaped}</span>`;
          case "command-invalid":
            return `<span class="text-twominal-fish-invalid font-bold underline decoration-wavy decoration-red-500">${escaped}</span>`;
          case "command-checking":
            return `<span class="dark:text-yellow-400 text-amber-600 font-bold">${escaped}</span>`;
          case "flag":
            return `<span class="text-twominal-fish-param">${escaped}</span>`;
          case "string":
            return `<span class="text-twominal-fish-quote">${escaped}</span>`;
          case "path":
            return `<span class="text-twominal-fish-path underline decoration-slate-400 dark:decoration-slate-600">${escaped}</span>`;
          case "variable":
            return `<span class="text-twominal-fish-keyword">${escaped}</span>`;
          case "operator":
            return `<span class="text-twominal-fish-keyword font-bold">${escaped}</span>`;
          case "comment":
            return `<span class="text-twominal-fish-comment italic">${escaped}</span>`;
          case "whitespace":
            return `<span class="whitespace-pre">${escaped}</span>`;
          case "argument":
          default:
            return `<span class="dark:text-slate-200 text-slate-800 font-mono">${escaped}</span>`;
        }
      })
      .join("");
  }
}

function escapeHtml(text: string): string {
  return text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#039;");
}

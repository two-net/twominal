export type TokenKind =
  | "cmd-ok"
  | "cmd-bad"
  | "string"
  | "comment"
  | "redirect"
  | "flag"
  | "text";

export type Token = { text: string; kind: TokenKind };

export const BUILTINS = new Set([
  "cd",
  "echo",
  "printf",
  "pwd",
  "export",
  "alias",
  "unalias",
  "true",
  "false",
  "exec",
  "eval",
  "set",
  "unset",
  "source",
  ".",
  "exit",
  "jobs",
  "fg",
  "bg",
  "wait",
  "umask",
  "ulimit",
  "hash",
  "type",
  "which",
  "command",
  "builtin",
  "let",
  "local",
  "return",
  "shift",
  "trap",
  "read",
  "test",
  "[",
  "[[",
  "time",
  "history",
  "pushd",
  "popd",
  "dirs",
  "kill",
  "bind",
  "complete",
  "help",
]);

const REDIR = /^(>>|>|<|2>|2>>|&>|&&|\|\||\||&)$/;

export function tokenize(line: string, commands: Set<string>): Token[] {
  const tokens: Token[] = [];
  let i = 0;
  let expectCmd = true;

  const push = (text: string, kind: TokenKind) => {
    if (text) tokens.push({ text, kind });
  };

  while (i < line.length) {
    const ch = line[i];
    if (ch === "#") {
      push(line.slice(i), "comment");
      break;
    }
    if (ch === " " || ch === "\t") {
      let j = i;
      while (j < line.length && (line[j] === " " || line[j] === "\t")) j++;
      push(line.slice(i, j), "text");
      i = j;
      continue;
    }
    if (ch === "'" || ch === '"') {
      const q = ch;
      let j = i + 1;
      while (j < line.length && line[j] !== q) {
        if (line[j] === "\\" && q === '"') j += 2;
        else j += 1;
      }
      if (j < line.length) j += 1;
      push(line.slice(i, j), "string");
      i = j;
      expectCmd = false;
      continue;
    }
    let j = i;
    while (j < line.length && !/\s/.test(line[j]) && line[j] !== "#" && line[j] !== "'" && line[j] !== '"') {
      j += 1;
    }
    const word = line.slice(i, j);
    if (REDIR.test(word) || word === ";") {
      push(word, "redirect");
      expectCmd = word === "|" || word === "||" || word === "&&" || word === ";";
    } else if (expectCmd) {
      const name = word.startsWith("\\") ? word.slice(1) : word;
      const base = name.split("/").pop() || name;
      const ok =
        commands.has(name) ||
        commands.has(base) ||
        BUILTINS.has(base) ||
        name.includes("/") ||
        name.startsWith(".");
      push(word, ok ? "cmd-ok" : "cmd-bad");
      expectCmd = false;
    } else if (word.startsWith("-")) {
      push(word, "flag");
    } else {
      push(word, "text");
    }
    i = j;
  }
  return tokens;
}

export function tokenAt(line: string, cursor: number): { start: number; end: number; text: string } {
  let start = cursor;
  while (start > 0 && !/\s/.test(line[start - 1])) start -= 1;
  let end = cursor;
  while (end < line.length && !/\s/.test(line[end])) end += 1;
  return { start, end, text: line.slice(start, end) };
}

export function firstWord(line: string): string {
  const t = line.trimStart();
  const m = t.match(/^[^\s]+/);
  return m ? m[0] : "";
}

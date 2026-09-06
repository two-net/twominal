import { invoke } from "@tauri-apps/api/core";
import { BUILTINS, firstWord, tokenAt } from "./highlight";

const FLAGS: Record<string, string[]> = {
  git: [
    "status",
    "add",
    "commit",
    "checkout",
    "push",
    "pull",
    "log",
    "diff",
    "branch",
    "switch",
    "merge",
    "clone",
    "init",
    "stash",
    "rebase",
    "fetch",
    "remote",
    "-m",
    "--amend",
    "--hard",
    "--oneline",
    "-u",
  ],
  ls: ["-l", "-a", "-la", "-h", "-R", "-1", "--color"],
  grep: ["-n", "-i", "-r", "-v", "-E", "-l", "--color"],
  rg: ["-n", "-i", "-t", "--files", "-g"],
  cat: ["-n", "-b"],
  cp: ["-r", "-i", "-v"],
  mv: ["-i", "-v"],
  rm: ["-r", "-f", "-i", "-v"],
  chmod: ["+x", "-R"],
  ssh: ["-i", "-L", "-p", "-A"],
  curl: ["-I", "-L", "-O", "-v", "-H"],
  npm: ["install", "run", "test", "build", "ci", "init"],
  cargo: ["build", "run", "test", "check", "clippy", "fmt", "install"],
};

const PREFERRED_CMDS = [
  "cd",
  "ls",
  "git",
  "npm",
  "npx",
  "node",
  "pnpm",
  "yarn",
  "bun",
  "cargo",
  "python3",
  "python",
  "pip3",
  "pip",
  "brew",
  "curl",
  "wget",
  "ssh",
  "cat",
  "less",
  "head",
  "tail",
  "grep",
  "rg",
  "find",
  "fd",
  "mkdir",
  "rm",
  "cp",
  "mv",
  "touch",
  "chmod",
  "vim",
  "nvim",
  "code",
  "docker",
  "kubectl",
  "make",
  "sudo",
  "man",
  "open",
  "echo",
  "pwd",
  "export",
  "clear",
  "exit",
  "history",
  "which",
];

const SKIP_CMDS = new Set([".", "[", "[["]);

type PathMemo = { cwd: string; prefix: string; hidden: boolean; items: string[]; at: number };
let pathMemo: PathMemo | null = null;

function isCmdPosition(line: string, start: number): boolean {
  const before = line.slice(0, start).trimEnd();
  if (!before) return true;
  return /(?:^|[;|&]|\|\||&&)\s*$/.test(before);
}

function isPathish(token: string): boolean {
  return (
    token.includes("/") ||
    token.includes("\\") ||
    token.startsWith("~") ||
    (token.startsWith(".") && token !== ".") ||
    token === ".."
  );
}

function filePrefix(prefix: string): string {
  const raw = prefix.replace(/\\/g, "/");
  const i = raw.lastIndexOf("/");
  return i >= 0 ? raw.slice(i + 1) : raw;
}

function stitch(line: string, start: number, match: string): string {
  return line.slice(0, start) + match;
}

function commandPool(commands: string[], abbrevs: Record<string, string>): string[] {
  const have = new Set(commands);
  for (const b of BUILTINS) have.add(b);
  const out: string[] = [];
  const seen = new Set<string>();
  const add = (s: string) => {
    if (!s || seen.has(s) || SKIP_CMDS.has(s)) return;
    if (!have.has(s) && !(s in abbrevs)) return;
    seen.add(s);
    out.push(s);
  };
  for (const c of PREFERRED_CMDS) add(c);
  for (const k of Object.keys(abbrevs)) add(k);
  for (const b of BUILTINS) add(b);
  for (const c of commands) add(c);
  return out;
}

function pickCommand(token: string, commands: string[], abbrevs: Record<string, string>): string | null {
  if (!token) return null;
  const pool = commandPool(commands, abbrevs);
  if (pool.includes(token)) return null;
  for (const item of pool) {
    if (item.startsWith(token)) return item;
  }
  return null;
}

function pickPath(token: string, items: string[], cmd: string): string | null {
  let hits = items.filter((i) => i.startsWith(token) && i.length > token.length);
  if (cmd === "cd") hits = hits.filter((i) => i.endsWith("/"));
  if (!hits.length) return null;
  if (token) {
    const exactDir = hits.find((i) => i === `${token}/`);
    if (exactDir) return exactDir;
  }
  hits.sort((a, b) => a.length - b.length || a.localeCompare(b));
  return hits[0];
}

async function completePaths(cwd: string, prefix: string): Promise<string[]> {
  const now = Date.now();
  const hidden = filePrefix(prefix).startsWith(".");
  if (
    pathMemo &&
    pathMemo.cwd === cwd &&
    pathMemo.hidden === hidden &&
    now - pathMemo.at < 3000 &&
    prefix.startsWith(pathMemo.prefix) &&
    pathMemo.items.length < 80
  ) {
    const needle = prefix.toLowerCase();
    return pathMemo.items.filter((i) => i.toLowerCase().startsWith(needle));
  }
  try {
    const items = await invoke<string[]>("complete_paths", { cwd, prefix });
    pathMemo = { cwd, prefix, hidden, items, at: now };
    return items;
  } catch {
    return [];
  }
}

export function needsPathSuggest(line: string): boolean {
  if (!line) return false;
  const tok = tokenAt(line, line.length);
  const token = tok.text;
  if (token.startsWith("-")) return false;
  if (isCmdPosition(line, tok.start) && !isPathish(token)) return false;
  return true;
}

export function suggestSync(
  line: string,
  commands: string[],
  abbrevs: Record<string, string> = {},
): string | null {
  if (!line) return null;
  const tok = tokenAt(line, line.length);
  const token = tok.text;
  if (token.startsWith("-")) {
    const flags = FLAGS[firstWord(line)] || [];
    const hit = flags.find((f) => f.startsWith(token) && f !== token);
    return hit ? stitch(line, tok.start, hit) : null;
  }
  if (isCmdPosition(line, tok.start) && !isPathish(token)) {
    const hit = pickCommand(token, commands, abbrevs);
    return hit ? stitch(line, tok.start, hit) : null;
  }
  if (token && !isPathish(token)) {
    const extra = (FLAGS[firstWord(line)] || []).filter((s) => !s.startsWith("-"));
    const hit = extra.find((s) => s.startsWith(token) && s !== token);
    if (hit) return stitch(line, tok.start, hit);
  }
  return null;
}

export async function suggestPath(line: string, cwd: string): Promise<string | null> {
  if (!needsPathSuggest(line)) return null;
  const tok = tokenAt(line, line.length);
  const items = await completePaths(cwd, tok.text);
  const hit = pickPath(tok.text, items, firstWord(line));
  if (!hit) return null;
  return stitch(line, tok.start, hit);
}

export async function complete(
  line: string,
  cursor: number,
  cwd: string,
  commands: string[],
  abbrevs: Record<string, string> = {},
): Promise<string[]> {
  const tok = tokenAt(line, cursor);
  const token = tok.text;
  if (token.startsWith("-")) {
    const cmd = firstWord(line);
    const flags = FLAGS[cmd] || [];
    return flags.filter((f) => f.startsWith(token));
  }
  if (isCmdPosition(line, tok.start) && !isPathish(token)) {
    const needle = token.toLowerCase();
    const abbrevHits = Object.keys(abbrevs).filter((k) => k.toLowerCase().startsWith(needle));
    const cmds = commands.filter((c) => c.toLowerCase().startsWith(needle));
    const builtinHits = [...BUILTINS].filter(
      (c) => !SKIP_CMDS.has(c) && c.toLowerCase().startsWith(needle),
    );
    const merged = [...new Set([...abbrevHits, ...cmds, ...builtinHits])].slice(0, 40);
    const extra = (FLAGS[token] ? FLAGS[token] : []).filter((s) => !s.startsWith("-"));
    if (merged.length) return merged;
    if (extra.length && !token) return extra;
  } else if (token && !isPathish(token)) {
    const extra = (FLAGS[firstWord(line)] || []).filter(
      (s) => !s.startsWith("-") && s.startsWith(token),
    );
    if (extra.length) return extra;
  }
  return completePaths(cwd, token);
}

export function applyCompletion(line: string, cursor: number, match: string): { line: string; cursor: number } {
  const tok = tokenAt(line, cursor);
  const next = line.slice(0, tok.start) + match + line.slice(tok.end);
  return { line: next, cursor: tok.start + match.length };
}

export function commonPrefix(items: string[]): string {
  if (!items.length) return "";
  let p = items[0];
  for (const s of items.slice(1)) {
    let i = 0;
    while (i < p.length && i < s.length && p[i] === s[i]) i++;
    p = p.slice(0, i);
  }
  return p;
}

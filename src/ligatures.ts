/** Longest-first programming ligatures for xterm character joiners. */
const SEQS = [
  "<!--",
  "<--",
  "!==",
  "===",
  "<=>",
  "...",
  ":::",
  ">>=",
  "<<=",
  "==>",
  "->",
  "<-",
  "=>",
  "<=",
  ">=",
  "!=",
  "==",
  "::",
  "++",
  "--",
  "&&",
  "||",
  "//",
  "/*",
  "*/",
  ":=",
  "??",
  "?.",
  "|>",
  "<|",
  ">>",
  "<<",
  "<>",
  "www",
].sort((a, b) => b.length - a.length);

export function ligatureRanges(text: string): [number, number][] {
  const ranges: [number, number][] = [];
  let i = 0;
  while (i < text.length) {
    let hit: string | null = null;
    for (const s of SEQS) {
      if (text.startsWith(s, i)) {
        hit = s;
        break;
      }
    }
    if (hit) {
      ranges.push([i, i + hit.length]);
      i += hit.length;
    } else {
      i += 1;
    }
  }
  return ranges;
}

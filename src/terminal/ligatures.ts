const PROGRAMMING_LIGATURES = [
  "<====>",
  "<--->",
  "<===>",
  "<!---",
  "===",
  "<--",
  "-->",
  "<==",
  "==>",
  "<->",
  "<=>",
  "<~~",
  "~~>",
  "<<<",
  ">>>",
  ":::",
  "+++",
  "->>",
  "<<-",
  "=>>",
  "<<=",
  "<*>",
  "<|>",
  "<!--",
  "!==",
  "<=",
  ">=",
  "==",
  "!=",
  "=>",
  "->",
  "<-",
  "::",
  ":=",
  "=:",
  "=~",
  "!~",
  "<>",
  "</",
  "/>",
  "/*",
  "*/",
  "|>",
  "<|",
] as const;

const LIGATURES_BY_LENGTH = [...new Set(PROGRAMMING_LIGATURES)].sort(
  (left, right) => right.length - left.length,
);

export function findLigatureRanges(text: string): [number, number][] {
  const ranges: [number, number][] = [];

  for (let index = 0; index < text.length; index += 1) {
    const ligature = LIGATURES_BY_LENGTH.find((candidate) =>
      text.startsWith(candidate, index),
    );
    if (!ligature) {
      continue;
    }

    ranges.push([index, index + ligature.length]);
    index += ligature.length - 1;
  }

  return ranges;
}

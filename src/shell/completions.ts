import {
  tokenizeShellLine,
  type ShellQuote,
  type TokenizedShellLine,
} from "./shellTokenizer";

export type CompletionRequestKind = "executable" | "path" | "environment";
export type CompletionCandidateKind =
  | "executable"
  | "directory"
  | "file"
  | "environment";

export interface CompletionRequest {
  readonly kind: CompletionRequestKind;
  readonly prefix: string;
  readonly cwd: string | null;
}

export interface NativeCompletionCandidate {
  readonly value: string;
  readonly display: string;
  readonly kind: CompletionCandidateKind;
}

export interface CompletionChoice extends NativeCompletionCandidate {
  readonly insertion: string;
}

export function completionRequestForLine(
  line: string,
  cursor: number,
  cwd: string | null,
): CompletionRequest | null {
  const parsed = tokenizeShellLine(line, cursor);
  const token = parsed.activeToken;
  if (
    cursor !== line.length ||
    token.suffix.length > 0 ||
    !token.closed ||
    token.kind === "operator" ||
    token.kind === "option"
  ) {
    return null;
  }

  if (token.kind === "environment") {
    const environment = environmentPrefix(token.prefix);
    return environment
      ? { kind: "environment", prefix: environment.name, cwd }
      : null;
  }

  if (token.commandPosition && token.kind !== "path") {
    return token.prefix
      ? { kind: "executable", prefix: token.prefix, cwd }
      : null;
  }

  return token.prefix
    ? { kind: "path", prefix: token.prefix, cwd }
    : null;
}

export function buildCompletionChoices(
  line: string,
  cursor: number,
  candidates: readonly NativeCompletionCandidate[],
): readonly CompletionChoice[] {
  const parsed = tokenizeShellLine(line, cursor);
  const token = parsed.activeToken;
  if (cursor !== line.length || token.suffix || !token.closed) {
    return [];
  }

  const seen = new Set<string>();
  const choices: CompletionChoice[] = [];
  const environment =
    token.kind === "environment" ? environmentPrefix(token.prefix) : null;
  for (const candidate of candidates) {
    if (candidate.kind === "environment") {
      if (
      !environment ||
        !candidate.value.startsWith(environment.name) ||
        candidate.value === environment.name ||
        containsUnsafeCandidateText(candidate.value) ||
        seen.has(candidate.value)
      ) {
        continue;
      }
      seen.add(candidate.value);
      choices.push({
        ...candidate,
        insertion:
          candidate.value.slice(environment.name.length) +
          environment.closing +
          " ",
      });
      continue;
    }
    if (
      !candidate.value.startsWith(token.prefix) ||
      candidate.value === token.prefix ||
      containsUnsafeCandidateText(candidate.value) ||
      seen.has(candidate.value)
    ) {
      continue;
    }

    seen.add(candidate.value);
    const suffix = candidate.value.slice(token.prefix.length);
    const insertionQuote = token.closed ? null : token.quote;
    choices.push({
      ...candidate,
      insertion:
        escapeCompletionSuffix(suffix, insertionQuote, candidate.kind) +
        completionTerminator(candidate.kind, insertionQuote),
    });
  }
  return choices.slice(0, 20);
}

interface EnvironmentPrefix {
  readonly name: string;
  readonly closing: string;
}

function environmentPrefix(value: string): EnvironmentPrefix | null {
  const powerShell = value.match(/^\$env:([A-Za-z_][A-Za-z0-9_]*)$/i);
  if (powerShell) {
    return { name: powerShell[1], closing: "" };
  }

  const braced = value.match(/^\$\{([A-Za-z_][A-Za-z0-9_]*)$/);
  if (braced) {
    return { name: braced[1], closing: "}" };
  }

  const posix = value.match(/^\$([A-Za-z_][A-Za-z0-9_]*)$/);
  if (posix) {
    return { name: posix[1], closing: "" };
  }

  const commandPrompt = value.match(/^%([A-Za-z_][A-Za-z0-9_]*)$/);
  return commandPrompt
    ? { name: commandPrompt[1], closing: "%" }
    : null;
}

export function syntaxContext(
  line: string,
  cursor: number = line.length,
): TokenizedShellLine["activeToken"] {
  return tokenizeShellLine(line, cursor).activeToken;
}

function escapeCompletionSuffix(
  value: string,
  quote: ShellQuote | null,
  kind: CompletionCandidateKind,
): string {
  if (kind === "environment") {
    return value;
  }
  if (quote === "single") {
    return value.replaceAll("'", "'\\''");
  }
  if (quote === "double") {
    return value.replace(/[\\"$`]/g, "\\$&");
  }
  return Array.from(value)
    .map((character) =>
      /\s/u.test(character) || "\\'\"$`|&;()<>[]{}*?!#~".includes(character)
        ? `\\${character}`
        : character,
    )
    .join("");
}

function completionTerminator(
  kind: CompletionCandidateKind,
  quote: ShellQuote | null,
): string {
  if (kind === "directory" || quote !== null) {
    return "";
  }
  return " ";
}

function containsUnsafeCandidateText(value: string): boolean {
  return Array.from(value).some((character) => {
    const codePoint = character.codePointAt(0) ?? 0;
    return (
      codePoint <= 0x1f ||
      (codePoint >= 0x7f && codePoint <= 0x9f) ||
      codePoint === 0x061c ||
      codePoint === 0x200e ||
      codePoint === 0x200f ||
      (codePoint >= 0x202a && codePoint <= 0x202e) ||
      (codePoint >= 0x2066 && codePoint <= 0x206f) ||
      codePoint === 0xfeff
    );
  });
}

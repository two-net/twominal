export type ShellTokenKind =
  | "command"
  | "argument"
  | "option"
  | "quoted"
  | "path"
  | "operator"
  | "environment";

export type ShellQuote = "single" | "double";

export interface ShellToken {
  readonly kind: ShellTokenKind;
  readonly text: string;
  readonly value: string;
  readonly start: number;
  readonly end: number;
  readonly quote: ShellQuote | null;
  readonly closed: boolean;
  readonly commandPosition: boolean;
  readonly redirectionTarget: boolean;
}

export interface ActiveShellToken {
  readonly tokenIndex: number | null;
  readonly kind: ShellTokenKind;
  readonly text: string;
  readonly value: string;
  readonly prefix: string;
  readonly suffix: string;
  readonly start: number;
  readonly end: number;
  readonly quote: ShellQuote | null;
  readonly closed: boolean;
  readonly commandPosition: boolean;
  readonly redirectionTarget: boolean;
}

export interface TokenizedShellLine {
  readonly line: string;
  readonly cursor: number;
  readonly tokens: readonly ShellToken[];
  readonly activeToken: ActiveShellToken;
}

interface LexicalWord {
  readonly type: "word";
  readonly text: string;
  readonly value: string;
  readonly start: number;
  readonly end: number;
  readonly quote: ShellQuote | null;
  readonly closed: boolean;
}

interface LexicalOperator {
  readonly type: "operator";
  readonly text: string;
  readonly start: number;
  readonly end: number;
}

type LexicalToken = LexicalWord | LexicalOperator;

const OPERATORS = [
  ";;&",
  "<<<",
  "<<-",
  "&>>",
  "&&",
  "||",
  "|&",
  ">>",
  "<<",
  "<>",
  ">|",
  "<&",
  ">&",
  "&>",
  ";;",
  ";&",
  "[[",
  "]]",
  "((",
  "))",
  "|",
  ";",
  "&",
  ">",
  "<",
  "(",
  ")",
] as const;

const COMMAND_SEPARATORS = new Set(["|", "|&", "||", "&&", ";", "&", "("]);
const REDIRECTION_PATTERN = /^(?:\d+)?(?:<<<|<<-|>>|<>|>\||<&|>&|>|<)/;
const ENVIRONMENT_ASSIGNMENT_PATTERN = /^[A-Za-z_][A-Za-z0-9_]*=/;

function operatorAt(line: string, index: number): string | null {
  const remainder = line.slice(index);
  const redirection = remainder.match(REDIRECTION_PATTERN)?.[0];
  const operator = OPERATORS.find((candidate) => remainder.startsWith(candidate));

  if (redirection && (!operator || redirection.length > operator.length)) {
    return redirection;
  }
  return operator ?? null;
}

function readWord(line: string, start: number): LexicalWord {
  let index = start;
  let value = "";
  let quoteState: ShellQuote | null = null;
  let firstQuote: ShellQuote | null = null;

  while (index < line.length) {
    const character = line[index];

    if (quoteState === "single") {
      if (character === "'") {
        quoteState = null;
      } else {
        value += character;
      }
      index += 1;
      continue;
    }

    if (quoteState === "double") {
      if (character === '"') {
        quoteState = null;
        index += 1;
        continue;
      }
      if (character === "\\") {
        if (index + 1 < line.length) {
          value += line[index + 1];
          index += 2;
        } else {
          value += character;
          index += 1;
        }
        continue;
      }
      value += character;
      index += 1;
      continue;
    }

    if (/\s/u.test(character)) {
      break;
    }
    if (operatorAt(line, index)) {
      break;
    }
    if (character === "\\") {
      if (index + 1 < line.length) {
        value += line[index + 1];
        index += 2;
      } else {
        value += character;
        index += 1;
      }
      continue;
    }
    if (character === "'" || character === '"') {
      quoteState = character === "'" ? "single" : "double";
      firstQuote ??= quoteState;
      index += 1;
      continue;
    }

    value += character;
    index += 1;
  }

  return {
    type: "word",
    text: line.slice(start, index),
    value,
    start,
    end: index,
    quote: firstQuote,
    closed: quoteState === null,
  };
}

function lex(line: string): LexicalToken[] {
  const tokens: LexicalToken[] = [];
  let index = 0;

  while (index < line.length) {
    if (/\s/u.test(line[index])) {
      index += 1;
      continue;
    }

    const operator = operatorAt(line, index);
    if (operator) {
      tokens.push({
        type: "operator",
        text: operator,
        start: index,
        end: index + operator.length,
      });
      index += operator.length;
      continue;
    }

    const word = readWord(line, index);
    tokens.push(word);
    index = word.end;
  }

  return tokens;
}

function isRedirection(operator: string): boolean {
  return REDIRECTION_PATTERN.test(operator) || operator === "&>" || operator === "&>>";
}

function isEnvironmentLike(value: string): boolean {
  return (
    ENVIRONMENT_ASSIGNMENT_PATTERN.test(value) ||
    /^\$(?:env:)?[A-Za-z_][A-Za-z0-9_]*/i.test(value) ||
    /^\$\{[A-Za-z_][A-Za-z0-9_]*\}?/.test(value) ||
    /^%[A-Za-z_][A-Za-z0-9_]*%?$/.test(value)
  );
}

function isPathLike(value: string): boolean {
  return (
    value === "." ||
    value === ".." ||
    value.includes("/") ||
    (value.includes("\\") && !value.endsWith("\\")) ||
    /^(?:\.{1,2}|~)\\/.test(value) ||
    /^~(?:[/\\]|$)/.test(value) ||
    /^[A-Za-z]:/.test(value)
  );
}

function classifyWord(
  word: LexicalWord,
  commandPosition: boolean,
  redirectionTarget: boolean,
): ShellTokenKind {
  if (word.quote) {
    return "quoted";
  }
  if (isEnvironmentLike(word.value)) {
    return "environment";
  }
  if (word.value === "--" || /^--?[^-\s]/.test(word.value)) {
    return "option";
  }
  if (isPathLike(word.value)) {
    return "path";
  }
  if (commandPosition) {
    return "command";
  }
  return redirectionTarget && isPathLike(word.value) ? "path" : "argument";
}

function classify(tokens: readonly LexicalToken[]): ShellToken[] {
  const classified: ShellToken[] = [];
  let expectsCommand = true;
  let expectsRedirectionTarget = false;

  for (const token of tokens) {
    if (token.type === "operator") {
      classified.push({
        kind: "operator",
        text: token.text,
        value: token.text,
        start: token.start,
        end: token.end,
        quote: null,
        closed: true,
        commandPosition: false,
        redirectionTarget: false,
      });

      if (COMMAND_SEPARATORS.has(token.text)) {
        expectsCommand = true;
        expectsRedirectionTarget = false;
      } else if (isRedirection(token.text)) {
        expectsRedirectionTarget = true;
      }
      continue;
    }

    const isAssignment = ENVIRONMENT_ASSIGNMENT_PATTERN.test(token.value);
    const redirectionTarget = expectsRedirectionTarget;
    const commandPosition = expectsCommand && !redirectionTarget && !isAssignment;
    classified.push({
      kind: classifyWord(token, commandPosition, redirectionTarget),
      text: token.text,
      value: token.value,
      start: token.start,
      end: token.end,
      quote: token.quote,
      closed: token.closed,
      commandPosition,
      redirectionTarget,
    });

    if (redirectionTarget) {
      expectsRedirectionTarget = false;
    } else if (commandPosition) {
      expectsCommand = false;
    }
  }

  return classified;
}

function decodeWordPrefix(text: string): string {
  if (!text) {
    return "";
  }
  return readWord(text, 0).value;
}

function emptyTokenContext(
  tokens: readonly ShellToken[],
  cursor: number,
): Pick<ActiveShellToken, "kind" | "commandPosition" | "redirectionTarget"> {
  let expectsCommand = true;
  let expectsRedirectionTarget = false;

  for (const token of tokens) {
    if (token.end > cursor) {
      break;
    }
    if (token.kind === "operator") {
      if (COMMAND_SEPARATORS.has(token.text)) {
        expectsCommand = true;
        expectsRedirectionTarget = false;
      } else if (isRedirection(token.text)) {
        expectsRedirectionTarget = true;
      }
      continue;
    }
    if (expectsRedirectionTarget) {
      expectsRedirectionTarget = false;
    } else if (token.commandPosition) {
      expectsCommand = false;
    }
  }

  return {
    kind: expectsCommand && !expectsRedirectionTarget ? "command" : "argument",
    commandPosition: expectsCommand && !expectsRedirectionTarget,
    redirectionTarget: expectsRedirectionTarget,
  };
}

function activeTokenAt(
  line: string,
  cursor: number,
  tokens: readonly ShellToken[],
): ActiveShellToken {
  const tokenIndexAtStart = tokens.findIndex(
    (token) => token.kind !== "operator" && token.start === cursor,
  );
  const tokenIndex =
    tokenIndexAtStart >= 0
      ? tokenIndexAtStart
      : tokens.findIndex(
          (token) =>
            token.kind !== "operator" && token.start < cursor && cursor <= token.end,
        );

  if (tokenIndex >= 0) {
    const token = tokens[tokenIndex];
    const rawPrefix = line.slice(token.start, cursor);
    return {
      tokenIndex,
      kind: token.kind,
      text: token.text,
      value: token.value,
      prefix: decodeWordPrefix(rawPrefix),
      suffix: line.slice(cursor, token.end),
      start: token.start,
      end: token.end,
      quote: token.quote,
      closed: token.closed,
      commandPosition: token.commandPosition,
      redirectionTarget: token.redirectionTarget,
    };
  }

  const context = emptyTokenContext(tokens, cursor);
  return {
    tokenIndex: null,
    ...context,
    text: "",
    value: "",
    prefix: "",
    suffix: "",
    start: cursor,
    end: cursor,
    quote: null,
    closed: true,
  };
}

export function tokenizeShellLine(
  line: string,
  cursor: number = line.length,
): TokenizedShellLine {
  const safeCursor = Number.isFinite(cursor)
    ? Math.max(0, Math.min(line.length, Math.trunc(cursor)))
    : 0;
  const tokens = classify(lex(line));

  return {
    line,
    cursor: safeCursor,
    tokens,
    activeToken: activeTokenAt(line, safeCursor, tokens),
  };
}

import { describe, expect, it } from "vitest";
import { tokenizeShellLine } from "./shellTokenizer";

describe("tokenizeShellLine", () => {
  it("classifies shell-like syntax without depending on a particular shell", () => {
    const parsed = tokenizeShellLine(
      'MODE=fast git --color "hello world" ./src | $PAGER',
    );

    expect(parsed.tokens.map(({ kind, value }) => ({ kind, value }))).toEqual([
      { kind: "environment", value: "MODE=fast" },
      { kind: "command", value: "git" },
      { kind: "option", value: "--color" },
      { kind: "quoted", value: "hello world" },
      { kind: "path", value: "./src" },
      { kind: "operator", value: "|" },
      { kind: "environment", value: "$PAGER" },
    ]);
    expect(parsed.tokens[1].commandPosition).toBe(true);
    expect(parsed.tokens[6].commandPosition).toBe(true);
  });

  it("tolerates unfinished quotes and trailing escapes", () => {
    const quote = tokenizeShellLine('echo "unfinished value');
    const escape = tokenizeShellLine("echo trailing\\");

    expect(quote.tokens[1]).toMatchObject({
      kind: "quoted",
      value: "unfinished value",
      quote: "double",
      closed: false,
    });
    expect(escape.tokens[1]).toMatchObject({
      kind: "argument",
      value: "trailing\\",
      closed: true,
    });
  });

  it("keeps whitespace and operators inside quotes in one token", () => {
    const parsed = tokenizeShellLine("printf 'one | two && three'|less");

    expect(parsed.tokens.map((token) => token.value)).toEqual([
      "printf",
      "one | two && three",
      "|",
      "less",
    ]);
    expect(parsed.tokens[3]).toMatchObject({
      kind: "command",
      commandPosition: true,
    });
  });

  it("reports a cursor-aware prefix, suffix, and replacement range", () => {
    const line = "git checkout";
    const parsed = tokenizeShellLine(line, 8);

    expect(parsed.activeToken).toMatchObject({
      tokenIndex: 1,
      kind: "argument",
      text: "checkout",
      value: "checkout",
      prefix: "chec",
      suffix: "kout",
      start: 4,
      end: 12,
      commandPosition: false,
    });
  });

  it("creates an empty command token after a command separator", () => {
    const parsed = tokenizeShellLine("build && ");

    expect(parsed.activeToken).toMatchObject({
      tokenIndex: null,
      kind: "command",
      prefix: "",
      start: 9,
      end: 9,
      commandPosition: true,
    });
  });

  it("tracks redirection targets without treating them as commands", () => {
    const parsed = tokenizeShellLine("echo 2>> logs/output.txt");

    expect(parsed.tokens[1]).toMatchObject({ kind: "operator", text: "2>>" });
    expect(parsed.tokens[2]).toMatchObject({
      kind: "path",
      redirectionTarget: true,
      commandPosition: false,
    });
  });

  it("recognizes Windows-style paths and environment references", () => {
    const parsed = tokenizeShellLine("open C:\\Users\\two %APPDATA%");

    expect(parsed.tokens[1].kind).toBe("path");
    expect(parsed.tokens[2].kind).toBe("environment");
    expect(tokenizeShellLine(".\\").tokens[0].kind).toBe("path");
  });

  it("clamps an invalid cursor to the line bounds", () => {
    expect(tokenizeShellLine("echo", Number.POSITIVE_INFINITY).cursor).toBe(0);
    expect(tokenizeShellLine("echo", 99).cursor).toBe(4);
  });
});

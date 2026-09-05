import { describe, expect, it } from "vitest";
import type { HistoryEntry } from "./history";
import {
  MAX_AUTOSUGGESTION_LIMIT,
  bestAutosuggestion,
  rankAutosuggestions,
} from "./autosuggestions";

describe("rankAutosuggestions", () => {
  it("returns only exact prefix continuations, most recent first", () => {
    const entries: HistoryEntry[] = [
      entry("echo git status", 100, 100),
      entry("Git status", 50, 99),
      entry("git status", 8, 7),
      entry("git stash", 1, 8),
    ];

    expect(rankAutosuggestions("git st", entries)).toEqual([
      {
        command: "git stash",
        suffix: "ash",
        frequency: 1,
        lastUsedSequence: 8,
      },
      {
        command: "git status",
        suffix: "atus",
        frequency: 8,
        lastUsedSequence: 7,
      },
    ]);
  });

  it("uses frequency then lexical order to break recency ties", () => {
    const entries: HistoryEntry[] = [
      entry("npm test:unit", 2, 10),
      entry("npm test:e2e", 9, 10),
      entry("npm test:a11y", 9, 10),
    ];

    expect(
      rankAutosuggestions("npm test:", entries).map(({ command }) => command),
    ).toEqual(["npm test:a11y", "npm test:e2e", "npm test:unit"]);
  });

  it("does not suggest an already complete command or an empty prefix", () => {
    const entries = [entry("pwd", 1, 1)];

    expect(rankAutosuggestions("pwd", entries)).toEqual([]);
    expect(rankAutosuggestions("", entries)).toEqual([]);
    expect(rankAutosuggestions("pw\u001bd", entries)).toEqual([]);
  });

  it("enforces requested and global result limits", () => {
    const entries = Array.from({ length: 100 }, (_, index) =>
      entry(`git command-${index}`, 1, index),
    );

    expect(rankAutosuggestions("git ", entries, { limit: 2 })).toHaveLength(2);
    expect(rankAutosuggestions("git ", entries, { limit: 0 })).toEqual([]);
    expect(rankAutosuggestions("git ", entries, { limit: 500 })).toHaveLength(
      MAX_AUTOSUGGESTION_LIMIT,
    );
  });
});

describe("bestAutosuggestion", () => {
  it("returns the best candidate or null", () => {
    const entries = [
      entry("cargo test", 1, 1),
      entry("cargo check", 1, 2),
    ];

    expect(bestAutosuggestion("cargo ", entries)?.command).toBe("cargo check");
    expect(bestAutosuggestion("npm ", entries)).toBeNull();
  });
});

function entry(
  command: string,
  frequency: number,
  lastUsedSequence: number,
): HistoryEntry {
  return { command, frequency, lastUsedSequence };
}

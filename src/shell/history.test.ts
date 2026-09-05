import { describe, expect, it } from "vitest";
import {
  HistoryNavigator,
  HistoryStore,
  MAX_HISTORY_CAPACITY,
  normalizeHistoryCommand,
} from "./history";

describe("normalizeHistoryCommand", () => {
  it("preserves ordinary Unicode commands and removes trailing whitespace", () => {
    expect(normalizeHistoryCommand("printf 'สวัสดี'   ")).toBe(
      "printf 'สวัสดี'",
    );
  });

  it.each([
    "",
    "   ",
    " hidden",
    "\thidden",
    "echo one\necho two",
    "echo one\recho two",
    "echo\u0000unsafe",
    "echo\u001bunsafe",
    "echo\u0085unsafe",
    "echo\u202eunsafe",
  ])("rejects unsafe or private-looking input %j", (value) => {
    expect(normalizeHistoryCommand(value)).toBeNull();
  });

  it("honors an explicit private marker without guessing from command words", () => {
    expect(normalizeHistoryCommand("login --password example")).toBe(
      "login --password example",
    );
    expect(
      normalizeHistoryCommand("echo ordinary", { private: true }),
    ).toBeNull();
  });
});

describe("HistoryStore", () => {
  it("bounds distinct commands and evicts the least recent entry", () => {
    const history = new HistoryStore({ capacity: 2 });
    history.record("first");
    history.record("second");
    history.record("third");

    expect(history.entries().map(({ command }) => command)).toEqual([
      "second",
      "third",
    ]);
  });

  it("deduplicates repeated commands while tracking frequency and recency", () => {
    const history = new HistoryStore({ capacity: 3 });
    history.record("git status");
    history.record("pwd");
    history.record("git status");

    expect(history.entries()).toEqual([
      { command: "pwd", frequency: 1, lastUsedSequence: 2 },
      { command: "git status", frequency: 2, lastUsedSequence: 3 },
    ]);
  });

  it("does not mutate history when a command is rejected", () => {
    const history = new HistoryStore();

    expect(history.record("safe command")).toBe(true);
    expect(history.record(" private command")).toBe(false);
    expect(history.record("private command", { private: true })).toBe(false);
    expect(history.size).toBe(1);
  });

  it("returns detached snapshots, recent entries, and clears state", () => {
    const history = new HistoryStore();
    history.record("one");
    history.record("two");
    const snapshot = history.entries();
    Reflect.set(snapshot[0], "command", "changed");

    expect(history.recent(1).map(({ command }) => command)).toEqual(["two"]);
    expect(history.entries()[0].command).toBe("one");

    history.clear();
    expect(history.size).toBe(0);
    expect(history.entries()).toEqual([]);
  });

  it.each([0, -1, 1.5, Number.NaN, MAX_HISTORY_CAPACITY + 1])(
    "rejects invalid capacity %s",
    (capacity) => {
      expect(() => new HistoryStore({ capacity })).toThrow(RangeError);
    },
  );
});

describe("HistoryNavigator", () => {
  it("moves through prefix matches and restores the original draft", () => {
    const history = new HistoryStore();
    history.record("git status");
    history.record("pwd");
    history.record("git stash");
    history.record("git log");
    const navigator = new HistoryNavigator(history);

    expect(navigator.previous("git st")).toBe("git stash");
    expect(navigator.previous("ignored while navigating")).toBe("git status");
    expect(navigator.previous("ignored while navigating")).toBe("git status");
    expect(navigator.next("ignored while navigating")).toBe("git stash");
    expect(navigator.next("ignored while navigating")).toBe("git st");
    expect(navigator.isNavigating).toBe(false);
  });

  it("returns an unmatched draft unchanged", () => {
    const history = new HistoryStore();
    history.record("git status");
    const navigator = new HistoryNavigator(history);

    expect(navigator.previous("npm")).toBe("npm");
    expect(navigator.next("npm install")).toBe("npm install");
  });

  it("uses a stable snapshot until navigation is reset", () => {
    const history = new HistoryStore();
    history.record("git status");
    const navigator = new HistoryNavigator(history);

    expect(navigator.previous("git ")).toBe("git status");
    history.record("git stash");
    expect(navigator.previous("ignored")).toBe("git status");

    navigator.reset();
    expect(navigator.previous("git ")).toBe("git stash");
  });

  it("allows empty-prefix navigation across all retained history", () => {
    const history = new HistoryStore();
    history.record("one");
    history.record("two");
    const navigator = new HistoryNavigator(history);

    expect(navigator.previous("")).toBe("two");
    expect(navigator.previous("ignored")).toBe("one");
    expect(navigator.next("ignored")).toBe("two");
    expect(navigator.next("ignored")).toBe("");
  });
});

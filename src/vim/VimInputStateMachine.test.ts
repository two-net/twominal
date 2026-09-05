import { describe, expect, it } from "vitest";
import {
  VimInputStateMachine,
  type VimInputResult,
  type VimKeyEvent,
  type VimLineSnapshot,
} from "./VimInputStateMachine";

describe("VimInputStateMachine", () => {
  it("starts each prompt in Insert mode at the requested cursor", () => {
    const line = "echo 👩🏽‍💻";
    const machine = new VimInputStateMachine({ line, cursor: line.length });
    expect(machine.state()).toEqual({
      line,
      cursor: line.length,
      mode: "insert",
      pendingCommand: null,
    });

    machine.beginPrompt({ line: "abc", cursor: 3 });
    expect(machine.state()).toMatchObject({ line: "abc", cursor: 3, mode: "insert" });
  });

  it("accepts Ctrl+[ as an Insert-mode escape", () => {
    const machine = new VimInputStateMachine();
    press(machine, "i", { line: "abc", cursor: 0 });
    expectEdit(
      machine.handleKey(
        { key: "[", ctrlKey: true },
        { line: "abc", cursor: 3 },
      ),
      { line: "abc", cursor: 2, mode: "normal" },
    );
  });

  it("implements i, a, A, and I insertion positions", () => {
    expectNormalCommand("i", { line: "abc", cursor: 1 }, 1, "insert");
    expectNormalCommand("a", { line: "abc", cursor: 1 }, 2, "insert");
    expectNormalCommand("A", { line: "abc", cursor: 1 }, 3, "insert");
    expectNormalCommand("I", { line: "  echo", cursor: 4 }, 2, "insert");
    expectNormalCommand("I", { line: "   ", cursor: 1 }, 0, "insert");
  });

  it("implements grapheme-aware horizontal and line motions", () => {
    const line = "a👩🏽‍💻b";
    const emojiStart = 1;
    const bStart = line.length - 1;

    expectNormalCommand("l", { line, cursor: 0 }, emojiStart);
    expectNormalCommand("l", { line, cursor: emojiStart }, bStart);
    expectNormalCommand("l", { line, cursor: bStart }, bStart);
    expectNormalCommand("h", { line, cursor: bStart }, emojiStart);
    expectNormalCommand("0", { line, cursor: bStart }, 0);
    expectNormalCommand("$", { line, cursor: 0 }, bStart);
  });

  it("implements Vim small-word w, b, and e motions", () => {
    const line = "one ++ two";

    expectNormalCommand("w", { line, cursor: 0 }, 4);
    expectNormalCommand("w", { line, cursor: 4 }, 7);
    expectNormalCommand("b", { line, cursor: 7 }, 4);
    expectNormalCommand("b", { line, cursor: 4 }, 0);
    expectNormalCommand("e", { line, cursor: 0 }, 2);
    expectNormalCommand("e", { line, cursor: 2 }, 5);
    expectNormalCommand("e", { line, cursor: 7 }, 9);
  });

  it("emits history intents for j and k", () => {
    const machine = normalMachine({ line: "git", cursor: 2 });

    expect(machine.handleKey(key("j"), { line: "git", cursor: 2 })).toEqual({
      kind: "history",
      mode: "normal",
      direction: "next",
    });
    expect(machine.handleKey(key("k"), { line: "git", cursor: 2 })).toEqual({
      kind: "history",
      mode: "normal",
      direction: "previous",
    });
  });

  it("deletes one extended grapheme with x", () => {
    const line = "a👩🏽‍💻b";
    const machine = normalMachine({ line, cursor: 1 });

    expectEdit(press(machine, "x", { line, cursor: 1 }), {
      line: "ab",
      cursor: 1,
      mode: "normal",
    });
  });

  it("keeps a valid Normal-mode cursor when x removes the final grapheme", () => {
    const machine = normalMachine({ line: "abc", cursor: 2 });

    expectEdit(press(machine, "x", { line: "abc", cursor: 2 }), {
      line: "ab",
      cursor: 1,
      mode: "normal",
    });
  });

  it("implements D and dd as undoable edits", () => {
    const deleteToEnd = normalMachine({ line: "echo value", cursor: 4 });
    expectEdit(
      press(deleteToEnd, "D", { line: "echo value", cursor: 4 }),
      { line: "echo", cursor: 3, mode: "normal" },
    );
    expectEdit(press(deleteToEnd, "u", { line: "echo", cursor: 3 }), {
      line: "echo value",
      cursor: 4,
      mode: "normal",
    });

    const deleteLine = normalMachine({ line: "pwd", cursor: 1 });
    press(deleteLine, "d", { line: "pwd", cursor: 1 });
    expect(deleteLine.state().pendingCommand).toBe("d");
    expectEdit(press(deleteLine, "d", { line: "pwd", cursor: 1 }), {
      line: "",
      cursor: 0,
      mode: "normal",
    });
    expectEdit(press(deleteLine, "u", { line: "", cursor: 0 }), {
      line: "pwd",
      cursor: 1,
      mode: "normal",
    });
  });

  it("resets an incomplete operator without applying the second key", () => {
    const machine = normalMachine({ line: "abc", cursor: 0 });
    press(machine, "d", { line: "abc", cursor: 0 });

    expectEdit(press(machine, "x", { line: "abc", cursor: 0 }), {
      line: "abc",
      cursor: 0,
      mode: "normal",
    });
    expect(machine.state().pendingCommand).toBeNull();
  });

  it("implements cw and groups the replacement into one undo step", () => {
    const original = { line: "git status", cursor: 4 };
    const machine = normalMachine(original);

    expectEdit(press(machine, "c", original), {
      ...original,
      mode: "normal",
    });
    expectEdit(press(machine, "w", original), {
      line: "git ",
      cursor: 4,
      mode: "insert",
    });

    machine.observe({ line: "git diff", cursor: 8 });
    expectEdit(press(machine, "Escape", { line: "git diff", cursor: 8 }), {
      line: "git diff",
      cursor: 7,
      mode: "normal",
    });
    expectEdit(press(machine, "u", { line: "git diff", cursor: 7 }), {
      line: "git status",
      cursor: 4,
      mode: "normal",
    });
  });

  it("implements ciw around the cursor without consuming surrounding spaces", () => {
    const original = { line: "echo status now", cursor: 7 };
    const machine = normalMachine(original);

    press(machine, "c", original);
    press(machine, "i", original);
    expect(machine.state().pendingCommand).toBe("ci");
    expectEdit(press(machine, "w", original), {
      line: "echo  now",
      cursor: 5,
      mode: "insert",
    });
  });

  it("selects the next word for ciw when the cursor is on whitespace", () => {
    const original = { line: "one   two", cursor: 3 };
    const machine = normalMachine(original);

    press(machine, "c", original);
    press(machine, "i", original);
    expectEdit(press(machine, "w", original), {
      line: "one   ",
      cursor: 6,
      mode: "insert",
    });
  });

  it("groups an entire Insert session for undo and Ctrl+r redo", () => {
    const machine = new VimInputStateMachine();
    press(machine, "i", { line: "", cursor: 0 });
    machine.observe({ line: "h", cursor: 1 });
    machine.observe({ line: "hello", cursor: 5 });
    press(machine, "Escape", { line: "hello", cursor: 5 });

    expectEdit(press(machine, "u", { line: "hello", cursor: 4 }), {
      line: "",
      cursor: 0,
      mode: "normal",
    });
    expectEdit(
      machine.handleKey(
        { key: "r", ctrlKey: true },
        { line: "", cursor: 0 },
      ),
      { line: "hello", cursor: 4, mode: "normal" },
    );
  });

  it("does not create an undo entry for insertion-mode movement alone", () => {
    const original = { line: "abc", cursor: 1 };
    const machine = normalMachine(original);
    press(machine, "a", original);
    press(machine, "Escape", { line: "abc", cursor: 2 });

    expectEdit(press(machine, "u", { line: "abc", cursor: 1 }), {
      line: "abc",
      cursor: 1,
      mode: "normal",
    });
  });

  it("passes terminal controls through while consuming unbound Normal-mode text", () => {
    const snapshot = { line: "sleep 10", cursor: 7 };
    const machine = normalMachine(snapshot);

    expect(machine.handleKey({ key: "c", ctrlKey: true }, snapshot)).toEqual({
      kind: "passthrough",
      mode: "normal",
    });
    expect(machine.handleKey(key("q"), snapshot)).toMatchObject({
      kind: "handled",
      mode: "normal",
      edit: snapshot,
    });
    expect(machine.handleKey(key("Enter"), snapshot)).toEqual({
      kind: "passthrough",
      mode: "normal",
    });
    expect(
      machine.handleKey({ key: "[", ctrlKey: true }, snapshot),
    ).toMatchObject({
      kind: "handled",
      mode: "normal",
    });
  });

  it("normalizes externally observed cursors to grapheme boundaries", () => {
    const line = "x👩🏽‍💻";
    const machine = normalMachine({ line, cursor: line.length });

    expect(machine.observe({ line, cursor: line.length })).toEqual({
      line,
      cursor: 1,
    });
    expect(machine.state()).toMatchObject({
      mode: "normal",
      line,
      cursor: 1,
    });
  });
});

function normalMachine(snapshot: VimLineSnapshot): VimInputStateMachine {
  const machine = new VimInputStateMachine(snapshot);
  machine.handleKey(key("Escape"), snapshot);
  return machine;
}

function expectNormalCommand(
  command: string,
  snapshot: VimLineSnapshot,
  cursor: number,
  mode: "insert" | "normal" = "normal",
): void {
  const machine = normalMachine(snapshot);
  expectEdit(press(machine, command, snapshot), {
    line: snapshot.line,
    cursor,
    mode,
  });
}

function press(
  machine: VimInputStateMachine,
  value: string,
  snapshot: VimLineSnapshot,
): VimInputResult {
  return machine.handleKey(key(value), snapshot);
}

function key(value: string): VimKeyEvent {
  return { key: value };
}

function expectEdit(
  result: VimInputResult,
  expected: VimLineSnapshot & { readonly mode: "insert" | "normal" },
): void {
  expect(result).toEqual({
    kind: "handled",
    mode: expected.mode,
    edit: { line: expected.line, cursor: expected.cursor },
  });
}

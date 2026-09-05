import { describe, expect, it } from "vitest";
import {
  isApplePlatform,
  tabShortcutFor,
  type TabShortcutEvent,
} from "./shortcuts";

const baseEvent: TabShortcutEvent = {
  code: "KeyT",
  altKey: false,
  ctrlKey: false,
  metaKey: false,
  shiftKey: false,
  isComposing: false,
  repeat: false,
};

function event(overrides: Partial<TabShortcutEvent>): TabShortcutEvent {
  return { ...baseEvent, ...overrides };
}

describe("tab shortcuts", () => {
  it("uses Command on Apple platforms and Control elsewhere", () => {
    expect(tabShortcutFor(event({ metaKey: true }), true)).toEqual({
      type: "new",
    });
    expect(tabShortcutFor(event({ ctrlKey: true }), false)).toEqual({
      type: "new",
    });
    expect(tabShortcutFor(event({ ctrlKey: true }), true)).toBeNull();
  });

  it("maps close, numeric selection, and relative navigation", () => {
    expect(
      tabShortcutFor(event({ code: "KeyW", ctrlKey: true }), false),
    ).toEqual({ type: "close" });
    expect(
      tabShortcutFor(event({ code: "Digit7", ctrlKey: true }), false),
    ).toEqual({ type: "activateIndex", index: 6 });
    expect(
      tabShortcutFor(
        event({ code: "BracketLeft", ctrlKey: true, shiftKey: true }),
        false,
      ),
    ).toEqual({ type: "previous" });
    expect(
      tabShortcutFor(
        event({ code: "BracketRight", metaKey: true, shiftKey: true }),
        true,
      ),
    ).toEqual({ type: "next" });
  });

  it("leaves composition, repeats, and unrelated modifiers untouched", () => {
    expect(tabShortcutFor(event({ metaKey: true, isComposing: true }), true)).toBeNull();
    expect(tabShortcutFor(event({ metaKey: true, repeat: true }), true)).toBeNull();
    expect(tabShortcutFor(event({ ctrlKey: true, altKey: true }), false)).toBeNull();
  });

  it("detects Apple platform names", () => {
    expect(isApplePlatform("MacIntel")).toBe(true);
    expect(isApplePlatform("iPhone")).toBe(true);
    expect(isApplePlatform("Win32")).toBe(false);
    expect(isApplePlatform("Linux x86_64")).toBe(false);
  });
});

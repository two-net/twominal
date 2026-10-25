import assert from "node:assert/strict";
import { test } from "node:test";

import { encodeKey } from "../../src/js/keys.js";

const key = (key, code = key, mods = {}) => ({
  key,
  code,
  ctrlKey: false,
  altKey: false,
  shiftKey: false,
  metaKey: false,
  ...mods,
});

test("editing keys", () => {
  assert.equal(encodeKey(key("Enter")), "\r");
  assert.equal(encodeKey(key("Backspace")), "\x7f");
  assert.equal(encodeKey(key("Tab")), "\t");
  assert.equal(encodeKey(key("Tab", "Tab", { shiftKey: true })), "\x1b[Z");
  assert.equal(encodeKey(key("Escape")), "\x1b");
  assert.equal(encodeKey(key("Delete")), "\x1b[3~");
  assert.equal(encodeKey(key("PageUp")), "\x1b[5~");
});

test("cursor keys follow application cursor mode and modifiers", () => {
  assert.equal(encodeKey(key("ArrowUp")), "\x1b[A");
  assert.equal(encodeKey(key("ArrowUp"), { appCursor: true }), "\x1bOA");
  assert.equal(encodeKey(key("ArrowLeft", "ArrowLeft", { shiftKey: true })), "\x1b[1;2D");
  assert.equal(encodeKey(key("ArrowRight", "ArrowRight", { altKey: true }), { appCursor: true }), "\x1b[1;3C");
  assert.equal(encodeKey(key("Home")), "\x1b[H");
});

test("function keys", () => {
  assert.equal(encodeKey(key("F1")), "\x1bOP");
  assert.equal(encodeKey(key("F5")), "\x1b[15~");
  assert.equal(encodeKey(key("F12", "F12", { ctrlKey: true })), "\x1b[24;5~");
});

test("control combinations", () => {
  assert.equal(encodeKey(key("c", "KeyC", { ctrlKey: true })), "\x03");
  assert.equal(encodeKey(key("q", "KeyA", { ctrlKey: true })), "\x11", "uses the layout's letter");
  assert.equal(encodeKey(key("[", "BracketLeft", { ctrlKey: true })), "\x1b");
  assert.equal(encodeKey(key(" ", "Space", { ctrlKey: true })), "\x00");
  assert.equal(encodeKey(key("Backspace", "Backspace", { ctrlKey: true })), "\x08");
});

test("Option is Meta only when the mode says so", () => {
  const optionB = key("∫", "KeyB", { altKey: true });
  assert.equal(encodeKey(optionB, { optionAsMeta: true }), "\x1bb");
  assert.equal(encodeKey(optionB, { optionAsMeta: false }), null, "types ∫ through the input element");
  assert.equal(encodeKey(key("¯", "Period", { altKey: true, shiftKey: true }), { optionAsMeta: true }), "\x1b>");
  assert.equal(encodeKey(key("Backspace", "Backspace", { altKey: true }), { optionAsMeta: true }), "\x1b\x7f");
});

test("printable text and ⌘ shortcuts are left alone", () => {
  assert.equal(encodeKey(key("a", "KeyA")), null);
  assert.equal(encodeKey(key("é", "KeyE")), null);
  assert.equal(encodeKey(key("t", "KeyT", { metaKey: true })), null);
  assert.equal(encodeKey(key("ArrowUp", "ArrowUp", { metaKey: true })), null);
});

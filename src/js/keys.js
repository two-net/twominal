// Encodes special keys for the terminal. Printable text arrives through the input element
// instead, so keyboard layouts, dead keys and input methods keep working (TERM-12).

const CSI = "\x1b[";
const SS3 = "\x1bO";

// Cursor keys: final byte, sent with SS3 in application cursor mode.
const CURSOR_KEYS = { ArrowUp: "A", ArrowDown: "B", ArrowRight: "C", ArrowLeft: "D", Home: "H", End: "F" };
// Keys sent as CSI <n> ~.
const TILDE_KEYS = {
  Insert: 2, Delete: 3, PageUp: 5, PageDown: 6,
  F5: 15, F6: 17, F7: 18, F8: 19, F9: 20, F10: 21, F11: 23, F12: 24,
};
const SS3_KEYS = { F1: "P", F2: "Q", F3: "R", F4: "S" };

// What a physical key types on a US layout, unshifted and shifted. Option-as-Meta sends these,
// because the event's own `key` is the special character Option would type.
const US_KEYS = {
  Backquote: "`~", Minus: "-_", Equal: "=+", BracketLeft: "[{", BracketRight: "]}", Backslash: "\\|",
  Semicolon: ";:", Quote: "'\"", Comma: ",<", Period: ".>", Slash: "/?", Space: "  ",
};
const SHIFTED_DIGITS = ")!@#$%^&*(";

/**
 * Returns the bytes for a key press, or null to leave it to the input element (or the menu).
 *
 * @param {{key: string, code: string, ctrlKey: boolean, altKey: boolean, shiftKey: boolean, metaKey: boolean}} event
 * @param {{appCursor?: boolean, optionAsMeta?: boolean}} mode
 */
export function encodeKey(event, mode = {}) {
  const { key, code, ctrlKey: ctrl, altKey: alt, shiftKey: shift, metaKey: cmd } = event;
  // ⌘ shortcuts belong to the menu and the system.
  if (cmd) return null;

  const meta = alt && Boolean(mode.optionAsMeta);
  const withMeta = (bytes) => (meta ? "\x1b" + bytes : bytes);
  const modifier = 1 + (shift ? 1 : 0) + (alt ? 2 : 0) + (ctrl ? 4 : 0);

  switch (key) {
    case "Enter":
      return withMeta("\r");
    case "Backspace":
      return withMeta(ctrl ? "\x08" : "\x7f");
    case "Tab":
      return shift ? CSI + "Z" : withMeta("\t");
    case "Escape":
      return "\x1b";
  }
  if (key in CURSOR_KEYS) {
    const final = CURSOR_KEYS[key];
    if (modifier > 1) return `${CSI}1;${modifier}${final}`;
    return (mode.appCursor ? SS3 : CSI) + final;
  }
  if (key in TILDE_KEYS) {
    const n = TILDE_KEYS[key];
    return modifier > 1 ? `${CSI}${n};${modifier}~` : `${CSI}${n}~`;
  }
  if (key in SS3_KEYS) {
    const final = SS3_KEYS[key];
    return modifier > 1 ? `${CSI}1;${modifier}${final}` : SS3 + final;
  }

  if (ctrl) {
    const control = controlChar(key, code);
    if (control !== null) return withMeta(control);
  }
  if (meta) {
    const base = usChar(code, shift);
    if (base !== null) return "\x1b" + base;
  }
  return null;
}

function controlChar(key, code) {
  // Prefer the layout's letter, so ⌃A is ⌃A on AZERTY too.
  const letter = /^[a-z]$/i.test(key) ? key.toUpperCase() : /^Key[A-Z]$/.test(code) ? code[3] : null;
  if (letter) return String.fromCharCode(letter.charCodeAt(0) - 64);
  switch (code) {
    case "Space":
    case "Digit2":
      return "\x00";
    case "BracketLeft":
    case "Digit3":
      return "\x1b";
    case "Backslash":
    case "Digit4":
      return "\x1c";
    case "BracketRight":
    case "Digit5":
      return "\x1d";
    case "Digit6":
      return "\x1e";
    case "Minus":
    case "Slash":
    case "Digit7":
      return "\x1f";
    case "Digit8":
      return "\x7f";
  }
  return null;
}

function usChar(code, shift) {
  if (/^Key[A-Z]$/.test(code)) return shift ? code[3] : code[3].toLowerCase();
  if (/^Digit\d$/.test(code)) return shift ? SHIFTED_DIGITS[Number(code[5])] : code[5];
  const pair = US_KEYS[code];
  return pair ? pair[shift ? 1 : 0] : null;
}

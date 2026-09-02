/**
 * Fast and robust ANSI escape sequence to HTML converter for Twominal
 */

const ANSI_COLORS: Record<number, string> = {
  30: "var(--ansi-black, #1e1e2e)", // black
  31: "var(--ansi-red, #f38ba8)", // red
  32: "var(--ansi-green, #a6e3a1)", // green
  33: "var(--ansi-yellow, #f9e2af)", // yellow
  34: "var(--ansi-blue, #89b4fa)", // blue
  35: "var(--ansi-magenta, #cba6f7)", // magenta
  36: "var(--ansi-cyan, #89dceb)", // cyan
  37: "var(--ansi-white, #cdd6f4)", // white
  90: "var(--ansi-bright-black, #585b70)", // bright black (gray)
  91: "var(--ansi-bright-red, #ff5555)", // bright red
  92: "var(--ansi-bright-green, #50fa7b)", // bright green
  93: "var(--ansi-bright-yellow, #f1fa8c)", // bright yellow
  94: "var(--ansi-bright-blue, #bd93f9)", // bright blue/purple
  95: "var(--ansi-bright-magenta, #ff79c6)", // bright magenta
  96: "var(--ansi-bright-cyan, #8be9fd)", // bright cyan
  97: "var(--ansi-bright-white, #ffffff)", // bright white
};

const ANSI_BG_COLORS: Record<number, string> = {
  40: "#11111b",
  41: "rgba(243, 139, 168, 0.25)",
  42: "rgba(166, 227, 161, 0.25)",
  43: "rgba(249, 226, 175, 0.25)",
  44: "rgba(137, 180, 250, 0.25)",
  45: "rgba(203, 166, 247, 0.25)",
  46: "rgba(137, 220, 235, 0.25)",
  47: "rgba(205, 214, 244, 0.25)",
  100: "rgba(88, 91, 112, 0.35)",
  101: "rgba(255, 85, 85, 0.3)",
  102: "rgba(80, 250, 123, 0.3)",
  103: "rgba(241, 250, 140, 0.3)",
  104: "rgba(189, 147, 249, 0.3)",
  105: "rgba(255, 121, 198, 0.3)",
  106: "rgba(139, 233, 253, 0.3)",
  107: "rgba(255, 255, 255, 0.3)",
};

export function escapeHtml(str: string): string {
  if (!str) return "";
  return str
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#039;");
}

export function ansiToHtml(raw: string): string {
  if (!raw) return "";

  // 1. Strip OSC sequences like window title changes: \x1b]0;...\x07 or \x1b]2;...\x1b\\
  let cleaned = raw.replace(/(?:\x1b|\u001b)\][0-9;]*[^\x07\x1b]*(?:\x07|\x1b\\)/g, "");

  // 2. Strip non-SGR CSI sequences (cursor movements, erase line, private modes: e.g. \x1b[?1h, \x1b[2K, \x1b[H)
  cleaned = cleaned.replace(/(?:\x1b|\u001b)\[[0-9;?<=>]*[A-LN-Za-ln-z~]/g, "");

  // 3. Strip 2-character / 3-character terminal escapes like \x1b=, \x1b>, \x1b(B, \x1bc
  cleaned = cleaned.replace(/(?:\x1b|\u001b)[()#%][0-9A-Za-z]/g, "");
  cleaned = cleaned.replace(/(?:\x1b|\u001b)[=><c]/g, "");

  // 4. Normalize \r\n to \n (CR LF -> LF)
  cleaned = cleaned.replace(/\r\n/g, "\n");

  // 5. Handle standalone carriage returns (e.g. spinners/progress: "frame 1\rframe 2")
  if (cleaned.includes("\r")) {
    const lines = cleaned.split("\n");
    cleaned = lines
      .map((line) => {
        if (line.includes("\r")) {
          const parts = line.split("\r").filter((p) => p.length > 0);
          return parts.length > 0 ? parts[parts.length - 1] : "";
        }
        return line;
      })
      .join("\n");
  }

  // 6. Regex for ANSI SGR codes: \x1b[...m or \u001b[...m
  const ansiRegex = /(?:\x1b|\u001b)\[([\d;]*)m/g;
  let html = "";
  let lastIndex = 0;

  let currentFg: string | null = null;
  let currentBg: string | null = null;
  let isBold = false;
  let isDim = false;
  let isItalic = false;
  let isUnderline = false;

  let match: RegExpExecArray | null;

  while ((match = ansiRegex.exec(cleaned)) !== null) {
    const textChunk = cleaned.slice(lastIndex, match.index);
    if (textChunk) {
      html += renderSpan(textChunk, currentFg, currentBg, isBold, isDim, isItalic, isUnderline);
    }
    lastIndex = ansiRegex.lastIndex;

    const codeStr = match[1] || "0";
    const codes = codeStr.split(";").map((c) => parseInt(c, 10) || 0);

    for (let i = 0; i < codes.length; i++) {
      const code = codes[i];
      if (code === 0) {
        // Reset
        currentFg = null;
        currentBg = null;
        isBold = false;
        isDim = false;
        isItalic = false;
        isUnderline = false;
      } else if (code === 1) {
        isBold = true;
      } else if (code === 2) {
        isDim = true;
      } else if (code === 3) {
        isItalic = true;
      } else if (code === 4) {
        isUnderline = true;
      } else if (code === 22) {
        isBold = false;
        isDim = false;
      } else if (code === 23) {
        isItalic = false;
      } else if (code === 24) {
        isUnderline = false;
      } else if (ANSI_COLORS[code]) {
        currentFg = ANSI_COLORS[code];
      } else if (ANSI_BG_COLORS[code]) {
        currentBg = ANSI_BG_COLORS[code];
      } else if (code === 39) {
        currentFg = null;
      } else if (code === 49) {
        currentBg = null;
      } else if (code === 38 && codes[i + 1] === 5) {
        // 256 color foreground
        const col = codes[i + 2] ?? 0;
        currentFg = get256Color(col);
        i += 2;
      } else if (code === 48 && codes[i + 1] === 5) {
        // 256 color background
        const col = codes[i + 2] ?? 0;
        currentBg = get256Color(col);
        i += 2;
      } else if (code === 38 && codes[i + 1] === 2) {
        // Truecolor foreground
        const r = codes[i + 2] ?? 0;
        const g = codes[i + 3] ?? 0;
        const b = codes[i + 4] ?? 0;
        currentFg = `rgb(${r},${g},${b})`;
        i += 4;
      } else if (code === 48 && codes[i + 1] === 2) {
        // Truecolor background
        const r = codes[i + 2] ?? 0;
        const g = codes[i + 3] ?? 0;
        const b = codes[i + 4] ?? 0;
        currentBg = `rgb(${r},${g},${b})`;
        i += 4;
      }
    }
  }

  // Trailing chunk
  const remaining = cleaned.slice(lastIndex);
  if (remaining) {
    html += renderSpan(remaining, currentFg, currentBg, isBold, isDim, isItalic, isUnderline);
  }

  return html;
}

function renderSpan(
  text: string,
  fg: string | null,
  bg: string | null,
  bold: boolean,
  dim: boolean,
  italic: boolean,
  underline: boolean
): string {
  // Strip any remaining non-SGR escape sequences or lone escape characters
  const cleaned = text
    .replace(/(?:\x1b|\u001b)\[[0-9;?<=>]*[A-Za-z~]/g, "")
    .replace(/(?:\x1b|\u001b)[=><c]/g, "")
    .replace(/(?:\x1b|\u001b)/g, "");
  if (!cleaned) return "";

  const escaped = escapeHtml(cleaned);

  const styles: string[] = [];
  if (fg) styles.push(`color: ${fg};`);
  if (bg) styles.push(`background-color: ${bg};`);
  if (bold) styles.push("font-weight: 700;");
  if (dim) styles.push("opacity: 0.65;");
  if (italic) styles.push("font-style: italic;");
  if (underline) styles.push("text-decoration: underline;");

  if (styles.length === 0) {
    return escaped;
  }

  return `<span style="${styles.join(" ")}">${escaped}</span>`;
}

function get256Color(n: number): string {
  if (n < 8) {
    const map = [
      "var(--ansi-black, #1e1e2e)",
      "var(--ansi-red, #f38ba8)",
      "var(--ansi-green, #a6e3a1)",
      "var(--ansi-yellow, #f9e2af)",
      "var(--ansi-blue, #89b4fa)",
      "var(--ansi-magenta, #cba6f7)",
      "var(--ansi-cyan, #89dceb)",
      "var(--ansi-white, #cdd6f4)"
    ];
    return map[n] || "var(--ansi-white, #cdd6f4)";
  }
  if (n < 16) {
    const map = [
      "var(--ansi-bright-black, #585b70)",
      "var(--ansi-bright-red, #ff5555)",
      "var(--ansi-bright-green, #50fa7b)",
      "var(--ansi-bright-yellow, #f1fa8c)",
      "var(--ansi-bright-blue, #bd93f9)",
      "var(--ansi-bright-magenta, #ff79c6)",
      "var(--ansi-bright-cyan, #8be9fd)",
      "var(--ansi-bright-white, #ffffff)"
    ];
    return map[n - 8] || "var(--ansi-bright-white, #ffffff)";
  }
  if (n < 232) {
    const idx = n - 16;
    const r = Math.floor(idx / 36) * 51;
    const g = Math.floor((idx % 36) / 6) * 51;
    const b = (idx % 6) * 51;
    return `rgb(${r},${g},${b})`;
  }
  const gray = (n - 232) * 10 + 8;
  return `rgb(${gray},${gray},${gray})`;
}

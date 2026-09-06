import { THEME_MODES, type Config } from "./config";

export type ResolvedTheme = "dark" | "light";

export const THEME_CYCLE: Config["theme"][] = THEME_MODES;

export const darkXterm = {
  background: "#0b0d12",
  foreground: "#e8edf5",
  cursor: "#5ee0b5",
  cursorAccent: "#0b0d12",
  selectionBackground: "#2a3f5c",
  black: "#12151c",
  red: "#ff6b8a",
  green: "#c3e88d",
  yellow: "#ffb86b",
  blue: "#82aaff",
  magenta: "#c792ea",
  cyan: "#5ee0b5",
  white: "#e8edf5",
  brightBlack: "#5c677a",
  brightRed: "#ff8ba4",
  brightGreen: "#d5f2a5",
  brightYellow: "#ffd19a",
  brightBlue: "#a8c4ff",
  brightMagenta: "#ddb6f2",
  brightCyan: "#8eebd0",
  brightWhite: "#ffffff",
};

export const lightXterm = {
  background: "#f4f6fb",
  foreground: "#1b2230",
  cursor: "#0f9d76",
  cursorAccent: "#f4f6fb",
  selectionBackground: "#c9d7ff",
  black: "#1b2230",
  red: "#d23b5a",
  green: "#3d8b40",
  yellow: "#c27800",
  blue: "#2451c7",
  magenta: "#6b3fa0",
  cyan: "#0f9d76",
  white: "#f4f6fb",
  brightBlack: "#5b6578",
  brightRed: "#e85b75",
  brightGreen: "#4eaa52",
  brightYellow: "#e09a20",
  brightBlue: "#3b6cff",
  brightMagenta: "#8b5cbf",
  brightCyan: "#1a9a8c",
  brightWhite: "#1b2230",
};

export function resolveTheme(cfg: Config): ResolvedTheme {
  switch (cfg.theme) {
    case "dark":
      return "dark";
    case "light":
      return "light";
    default: {
      const prefersDark = window.matchMedia("(prefers-color-scheme: dark)").matches;
      return prefersDark ? "dark" : "light";
    }
  }
}

export function themeStatusLabel(cfg: Config): string {
  return cfg.theme;
}

export function nextTheme(current: Config["theme"]): Config["theme"] {
  const i = THEME_CYCLE.indexOf(current);
  return THEME_CYCLE[(i < 0 ? 0 : i + 1) % THEME_CYCLE.length];
}

export function applyChrome(cfg: Config, theme: ResolvedTheme): void {
  const root = document.documentElement;
  const osReduce = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  root.dataset.theme = theme;
  root.classList.toggle("reduce-motion", cfg.reduceMotion || osReduce);
  root.style.setProperty("--font-mono", `"${cfg.fontFamily}", "JetBrains Mono", "Fira Code", ui-monospace, monospace`);
  root.style.setProperty("--font-size", `${cfg.fontSize}px`);
  root.style.setProperty("--liga", cfg.ligatures ? `"calt" 1, "liga" 1` : `"calt" 0, "liga" 0`);
  root.dataset.liga = cfg.ligatures ? "on" : "off";
}

export type ThemeMode = "system" | "dark" | "light";

export const THEME_MODES: ThemeMode[] = ["dark", "light", "system"];

export interface Config {
  theme: ThemeMode;
  fontFamily: string;
  fontSize: number;
  ligatures: boolean;
  shell: string | null;
  vimMode: boolean;
  reduceMotion: boolean;
  rawPty: boolean;
  abbreviations: Record<string, string>;
}

export function normalizeTheme(value: unknown): ThemeMode {
  return THEME_MODES.includes(value as ThemeMode) ? (value as ThemeMode) : "system";
}

export const FONT_CHOICES = [
  "JetBrains Mono",
  "Fira Code",
  "Cascadia Code",
  "Menlo",
  "Consolas",
  "ui-monospace",
];

export function defaultConfig(): Config {
  return {
    theme: "system",
    fontFamily: "JetBrains Mono",
    fontSize: 13.5,
    ligatures: true,
    shell: null,
    vimMode: true,
    reduceMotion: false,
    rawPty: false,
    abbreviations: { ll: "ls -la", gco: "git checkout", gs: "git status" },
  };
}

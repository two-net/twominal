export const APP_CONFIG_SCHEMA_VERSION = 1 as const;

export const DEFAULT_TERMINAL_FONT_FAMILY =
  "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, 'Liberation Mono', monospace";

export type AppearanceMode = "system" | "light" | "dark" | "sunSchedule";

export interface AppearanceConfig {
  mode: AppearanceMode;
  latitude: number | null;
  longitude: number | null;
}

export interface TerminalConfig {
  fontFamily: string;
  fontSize: number;
  lineHeight: number;
  letterSpacing: number;
  fontWeight: number;
  fontLigatures: boolean;
}

export interface AppConfig {
  schemaVersion: typeof APP_CONFIG_SCHEMA_VERSION;
  appearance: AppearanceConfig;
  terminal: TerminalConfig;
  vimMode: boolean;
  animations: boolean;
}

export const DEFAULT_APP_CONFIG = {
  schemaVersion: APP_CONFIG_SCHEMA_VERSION,
  appearance: {
    mode: "system",
    latitude: null,
    longitude: null,
  },
  terminal: {
    fontFamily: DEFAULT_TERMINAL_FONT_FAMILY,
    fontSize: 14,
    lineHeight: 1.18,
    letterSpacing: 0,
    fontWeight: 400,
    fontLigatures: true,
  },
  vimMode: false,
  animations: true,
} as const satisfies AppConfig;

export function createDefaultAppConfig(): AppConfig {
  return {
    schemaVersion: DEFAULT_APP_CONFIG.schemaVersion,
    appearance: { ...DEFAULT_APP_CONFIG.appearance },
    terminal: { ...DEFAULT_APP_CONFIG.terminal },
    vimMode: DEFAULT_APP_CONFIG.vimMode,
    animations: DEFAULT_APP_CONFIG.animations,
  };
}

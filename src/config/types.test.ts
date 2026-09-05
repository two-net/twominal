import { describe, expect, it } from "vitest";
import {
  APP_CONFIG_SCHEMA_VERSION,
  DEFAULT_APP_CONFIG,
  DEFAULT_TERMINAL_FONT_FAMILY,
  createDefaultAppConfig,
} from "./types";

describe("application config defaults", () => {
  it("matches the native schema and stable application defaults", () => {
    expect(DEFAULT_APP_CONFIG).toEqual({
      schemaVersion: APP_CONFIG_SCHEMA_VERSION,
      appearance: {
        mode: "system",
        latitude: null,
        longitude: null,
      },
      terminal: {
        fontFamily:
          "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, 'Liberation Mono', monospace",
        fontSize: 14,
        lineHeight: 1.18,
        letterSpacing: 0,
        fontWeight: 400,
        fontLigatures: true,
      },
      vimMode: false,
      animations: true,
    });
    expect(DEFAULT_TERMINAL_FONT_FAMILY).toBe(
      DEFAULT_APP_CONFIG.terminal.fontFamily,
    );
  });

  it("creates independent mutable config trees", () => {
    const first = createDefaultAppConfig();
    const second = createDefaultAppConfig();

    first.appearance.mode = "dark";
    first.terminal.fontSize = 18;
    first.vimMode = true;

    expect(second.appearance.mode).toBe("system");
    expect(second.terminal.fontSize).toBe(14);
    expect(second.vimMode).toBe(false);
    expect(DEFAULT_APP_CONFIG.appearance.mode).toBe("system");
  });
});

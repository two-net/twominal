import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  ConfigClientError,
  TauriConfigClient,
  normalizeConfigError,
} from "./configClient";
import { createDefaultAppConfig } from "./types";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

const invokeMock = vi.mocked(invoke);

describe("TauriConfigClient", () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it("loads configuration through the narrow config command", async () => {
    const config = createDefaultAppConfig();
    invokeMock.mockResolvedValue(config);

    await expect(new TauriConfigClient().load()).resolves.toBe(config);
    expect(invokeMock).toHaveBeenCalledWith("config_load");
  });

  it("saves one typed config payload and returns the normalized result", async () => {
    const config = createDefaultAppConfig();
    const saved = {
      ...config,
      terminal: { ...config.terminal, fontSize: 16 },
    };
    invokeMock.mockResolvedValue(saved);

    await expect(new TauriConfigClient().save(config)).resolves.toBe(saved);
    expect(invokeMock).toHaveBeenCalledWith("config_save", { config });
  });

  it("preserves native command error codes without leaking raw objects", async () => {
    invokeMock.mockRejectedValue({
      code: "config_invalid",
      message: "Font size is outside the supported range.",
      internalPath: "/private/settings.json",
    });

    const failure = await new TauriConfigClient().save(
      createDefaultAppConfig(),
    ).catch((error: unknown) => error);

    expect(failure).toBeInstanceOf(ConfigClientError);
    expect(failure).toMatchObject({
      code: "config_invalid",
      message: "Font size is outside the supported range.",
    });
    expect(failure).not.toHaveProperty("internalPath");
  });
});

describe("normalizeConfigError", () => {
  it.each([
    [new Error("disk unavailable"), "disk unavailable"],
    ["permission denied", "permission denied"],
    [null, "fallback"],
  ])("normalizes unknown failures", (input, expectedMessage) => {
    expect(normalizeConfigError(input, "fallback")).toMatchObject({
      name: "ConfigClientError",
      code: "config_unavailable",
      message: expectedMessage,
    });
  });
});

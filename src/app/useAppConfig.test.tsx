import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ConfigClient } from "../config/configClient";
import { createDefaultAppConfig } from "../config/types";
import { useAppConfig } from "./useAppConfig";

afterEach(cleanup);

describe("useAppConfig", () => {
  it("loads native settings without writing them back", async () => {
    const loaded = createDefaultAppConfig();
    loaded.appearance.mode = "dark";
    const client = createClient({ loaded });
    const { result } = renderHook(() => useAppConfig(client));

    await waitFor(() => expect(result.current.ready).toBe(true));
    expect(result.current.config.appearance.mode).toBe("dark");
    expect(result.current.saveStatus).toBe("saved");
    expect(client.save).not.toHaveBeenCalled();
  });

  it("debounces and persists settings changes", async () => {
    const client = createClient();
    const { result } = renderHook(() => useAppConfig(client));
    await waitFor(() => expect(result.current.ready).toBe(true));

    act(() => {
      result.current.setConfig((config) => ({
        ...config,
        terminal: { ...config.terminal, fontSize: 17 },
      }));
    });
    expect(result.current.saveStatus).toBe("saving");

    await waitFor(() => expect(client.save).toHaveBeenCalledOnce());
    expect(client.save).toHaveBeenCalledWith(
      expect.objectContaining({
        terminal: expect.objectContaining({ fontSize: 17 }),
      }),
    );
    await waitFor(() => expect(result.current.saveStatus).toBe("saved"));
  });

  it("does not overwrite an unreadable file until the user changes settings", async () => {
    const client = createClient({ loadError: new Error("invalid config") });
    const { result } = renderHook(() => useAppConfig(client));

    await waitFor(() => expect(result.current.ready).toBe(true));
    expect(result.current.saveStatus).toBe("error");
    expect(result.current.errorMessage).toBe("invalid config");
    expect(client.save).not.toHaveBeenCalled();

    act(() => {
      result.current.setConfig((config) => ({
        ...config,
        animations: false,
      }));
    });
    await waitFor(() => expect(client.save).toHaveBeenCalledOnce());
  });
});

function createClient({
  loaded = createDefaultAppConfig(),
  loadError,
}: {
  loaded?: ReturnType<typeof createDefaultAppConfig>;
  loadError?: unknown;
} = {}): ConfigClient & {
  load: ReturnType<typeof vi.fn<ConfigClient["load"]>>;
  save: ReturnType<typeof vi.fn<ConfigClient["save"]>>;
} {
  return {
    load: vi.fn(() =>
      loadError === undefined
        ? Promise.resolve(loaded)
        : Promise.reject(loadError),
    ),
    save: vi.fn((config) => Promise.resolve(config)),
  };
}

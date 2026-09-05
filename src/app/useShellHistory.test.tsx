import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { HistoryEntry, ShellClient } from "../shell";
import { useShellHistory } from "./useShellHistory";

afterEach(cleanup);

describe("useShellHistory", () => {
  it("loads shared history and serializes command updates", async () => {
    const client = createClient([
      { command: "pwd", frequency: 1, lastUsedSequence: 1 },
    ]);
    const { result } = renderHook(() => useShellHistory(client));
    await waitFor(() => expect(result.current.status).toBe("ready"));

    act(() => {
      result.current.recordCommand("git status");
      result.current.recordCommand("git status");
    });
    await waitFor(() => expect(client.appendHistory).toHaveBeenCalledTimes(2));
    await waitFor(() =>
      expect(result.current.entries.at(-1)).toMatchObject({
        command: "git status",
        frequency: 2,
      }),
    );
  });

  it("clears native and in-memory history", async () => {
    const client = createClient([
      { command: "pwd", frequency: 1, lastUsedSequence: 1 },
    ]);
    const { result } = renderHook(() => useShellHistory(client));
    await waitFor(() => expect(result.current.entries).toHaveLength(1));

    await act(() => result.current.clearHistory());
    expect(client.clearHistory).toHaveBeenCalledOnce();
    expect(result.current.entries).toEqual([]);
  });

  it("preserves file order when persisted timestamps collide", async () => {
    const client = createClient([
      { command: "older", frequency: 1, lastUsedSequence: 42 },
      { command: "newer", frequency: 1, lastUsedSequence: 42 },
    ]);
    const { result } = renderHook(() => useShellHistory(client));

    await waitFor(() => expect(result.current.status).toBe("ready"));
    expect(result.current.entries.map((entry) => entry.lastUsedSequence)).toEqual([
      1, 2,
    ]);
  });

  it("surfaces load errors and retries without writing history", async () => {
    const client = createClient([]);
    vi.mocked(client.loadHistory)
      .mockRejectedValueOnce(new Error("history unavailable"))
      .mockResolvedValueOnce([]);
    const { result } = renderHook(() => useShellHistory(client));
    await waitFor(() => expect(result.current.status).toBe("error"));
    expect(result.current.errorMessage).toBe("history unavailable");

    act(() => result.current.retryLoad());
    await waitFor(() => expect(result.current.status).toBe("ready"));
    expect(client.appendHistory).not.toHaveBeenCalled();
  });
});

function createClient(initial: readonly HistoryEntry[]): ShellClient & {
  loadHistory: ReturnType<typeof vi.fn<ShellClient["loadHistory"]>>;
  appendHistory: ReturnType<typeof vi.fn<ShellClient["appendHistory"]>>;
  clearHistory: ReturnType<typeof vi.fn<ShellClient["clearHistory"]>>;
  complete: ReturnType<typeof vi.fn<ShellClient["complete"]>>;
} {
  let useCount = 0;
  return {
    loadHistory: vi.fn(() => Promise.resolve(initial)),
    appendHistory: vi.fn((command) => {
      useCount += 1;
      return Promise.resolve({
        command,
        frequency: useCount,
        lastUsedSequence: useCount + 1,
      });
    }),
    clearHistory: vi.fn(() => Promise.resolve()),
    complete: vi.fn(() => Promise.resolve([])),
  };
}

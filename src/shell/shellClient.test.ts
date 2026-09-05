import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ShellClientError } from "./shellClient";
import {
  TauriShellClient,
  normalizeShellError,
} from "./shellClient";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const invokeMock = vi.mocked(invoke);

describe("TauriShellClient", () => {
  beforeEach(() => invokeMock.mockReset());

  it("maps bounded native history stats to the ranking model", async () => {
    invokeMock.mockResolvedValue([
      { command: "git status", lastUsedAtMs: 42, useCount: 3 },
    ]);

    await expect(new TauriShellClient().loadHistory()).resolves.toEqual([
      { command: "git status", lastUsedSequence: 42, frequency: 3 },
    ]);
    expect(invokeMock).toHaveBeenCalledWith("history_load");
  });

  it("uses narrow append, clear, and session-bound completion commands", async () => {
    const client = new TauriShellClient();
    invokeMock.mockResolvedValueOnce(null);
    await expect(client.appendHistory(" private")).resolves.toBeNull();
    expect(invokeMock).toHaveBeenLastCalledWith("history_append", {
      command: " private",
    });

    invokeMock.mockResolvedValueOnce(undefined);
    await client.clearHistory();
    expect(invokeMock).toHaveBeenLastCalledWith("history_clear");

    invokeMock.mockResolvedValueOnce([
      { value: "status", display: "status", kind: "file" },
    ]);
    await expect(
      client.complete("session", {
        kind: "path",
        prefix: "sta",
        cwd: "/tmp",
      }),
    ).resolves.toHaveLength(1);
    expect(invokeMock).toHaveBeenLastCalledWith("completion_query", {
      sessionId: "session",
      request: { kind: "path", prefix: "sta", cwd: "/tmp" },
    });
  });

  it("rejects malformed responses", async () => {
    invokeMock.mockResolvedValue([{ command: "pwd", useCount: 0 }]);
    await expect(new TauriShellClient().loadHistory()).rejects.toMatchObject({
      code: "shell_invalid_response",
    });

    invokeMock.mockResolvedValue([
      { command: "echo\u202eunsafe", lastUsedAtMs: 1, useCount: 1 },
    ]);
    await expect(new TauriShellClient().loadHistory()).rejects.toMatchObject({
      code: "shell_invalid_response",
    });
  });

  it("sanitizes native errors", () => {
    const error = normalizeShellError({
      code: "history_read_failed",
      message: "History unavailable.",
      privatePath: "/secret/history.json",
    }, "fallback");
    const shellError = error as ShellClientError;
    expect(shellError.name).toBe("ShellClientError");
    expect(shellError.code).toBe("history_read_failed");
    expect(shellError.message).toBe("History unavailable.");
    expect(Reflect.has(shellError, "privatePath")).toBe(false);
    expect(shellError.cause).toBeUndefined();
  });
});

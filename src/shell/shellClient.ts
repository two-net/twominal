import { invoke } from "@tauri-apps/api/core";
import type {
  CompletionRequest,
  NativeCompletionCandidate,
} from "./completions";
import { normalizeHistoryCommand, type HistoryEntry } from "./history";

export interface ShellClient {
  loadHistory(): Promise<readonly HistoryEntry[]>;
  appendHistory(command: string): Promise<HistoryEntry | null>;
  clearHistory(): Promise<void>;
  complete(
    sessionId: string,
    request: CompletionRequest,
  ): Promise<readonly NativeCompletionCandidate[]>;
}

export class ShellClientError extends Error {
  readonly code: string;

  constructor(code: string, message: string, cause?: unknown) {
    super(message, cause === undefined ? undefined : { cause });
    this.name = "ShellClientError";
    this.code = code;
  }
}

export class TauriShellClient implements ShellClient {
  async loadHistory(): Promise<readonly HistoryEntry[]> {
    try {
      return parseHistoryEntries(await invoke<unknown>("history_load"));
    } catch (error) {
      throw normalizeShellError(
        error,
        "Twominal could not load command history.",
      );
    }
  }

  async appendHistory(command: string): Promise<HistoryEntry | null> {
    try {
      const result = await invoke<unknown>("history_append", { command });
      return result === null ? null : parseHistoryEntry(result);
    } catch (error) {
      throw normalizeShellError(
        error,
        "Twominal could not save command history.",
      );
    }
  }

  async clearHistory(): Promise<void> {
    try {
      await invoke<void>("history_clear");
    } catch (error) {
      throw normalizeShellError(
        error,
        "Twominal could not clear command history.",
      );
    }
  }

  async complete(
    sessionId: string,
    request: CompletionRequest,
  ): Promise<readonly NativeCompletionCandidate[]> {
    try {
      return parseCompletionCandidates(
        await invoke<unknown>("completion_query", { sessionId, request }),
      );
    } catch (error) {
      throw normalizeShellError(error, "Completions are temporarily unavailable.");
    }
  }
}

export function normalizeShellError(
  error: unknown,
  fallback: string,
): ShellClientError {
  if (error instanceof ShellClientError) {
    return error;
  }
  if (isRecord(error)) {
    const code =
      typeof error.code === "string" && error.code
        ? error.code
        : "shell_experience_unavailable";
    const message =
      typeof error.message === "string" && error.message
        ? error.message
        : fallback;
    return new ShellClientError(code, message);
  }
  if (error instanceof Error) {
    return new ShellClientError(
      "shell_experience_unavailable",
      error.message || fallback,
      error,
    );
  }
  return new ShellClientError(
    "shell_experience_unavailable",
    typeof error === "string" && error ? error : fallback,
    error,
  );
}

function parseHistoryEntries(value: unknown): readonly HistoryEntry[] {
  if (!Array.isArray(value) || value.length > 1_000) {
    throw invalidResponse();
  }
  return value.map(parseHistoryEntry);
}

function parseHistoryEntry(value: unknown): HistoryEntry {
  if (
    !isRecord(value) ||
    typeof value.command !== "string" ||
    normalizeHistoryCommand(value.command) !== value.command ||
    !Number.isSafeInteger(value.lastUsedAtMs) ||
    (value.lastUsedAtMs as number) < 0 ||
    !Number.isSafeInteger(value.useCount) ||
    (value.useCount as number) < 1
  ) {
    throw invalidResponse();
  }
  return {
    command: value.command,
    frequency: value.useCount as number,
    lastUsedSequence: value.lastUsedAtMs as number,
  };
}

function parseCompletionCandidates(
  value: unknown,
): readonly NativeCompletionCandidate[] {
  if (!Array.isArray(value) || value.length > 100) {
    throw invalidResponse();
  }
  return value.map((candidate) => {
    if (
      !isRecord(candidate) ||
      typeof candidate.value !== "string" ||
      typeof candidate.display !== "string" ||
      !isCompletionKind(candidate.kind) ||
      containsUnsafeText(candidate.value) ||
      containsUnsafeText(candidate.display) ||
      utf8Length(candidate.value) > 4_096 ||
      utf8Length(candidate.display) > 4_096
    ) {
      throw invalidResponse();
    }
    return {
      value: candidate.value,
      display: candidate.display,
      kind: candidate.kind,
    };
  });
}

function utf8Length(value: string): number {
  return new TextEncoder().encode(value).byteLength;
}

function containsUnsafeText(value: string): boolean {
  return Array.from(value).some((character) => {
    const point = character.codePointAt(0) ?? 0;
    return (
      point <= 0x1f ||
      (point >= 0x7f && point <= 0x9f) ||
      point === 0x061c ||
      point === 0x200e ||
      point === 0x200f ||
      (point >= 0x202a && point <= 0x202e) ||
      (point >= 0x2066 && point <= 0x206f) ||
      point === 0xfeff
    );
  });
}

function isCompletionKind(
  value: unknown,
): value is NativeCompletionCandidate["kind"] {
  return (
    value === "executable" ||
    value === "directory" ||
    value === "file" ||
    value === "environment"
  );
}

function invalidResponse(): ShellClientError {
  return new ShellClientError(
    "shell_invalid_response",
    "Twominal received an invalid shell-experience response.",
  );
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

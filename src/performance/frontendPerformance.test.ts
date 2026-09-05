import { describe, expect, it, vi } from "vitest";
import {
  buildCompletionChoices,
  completionRequestForLine,
  type NativeCompletionCandidate,
} from "../shell/completions";
import { rankAutosuggestions } from "../shell/autosuggestions";
import type { HistoryEntry } from "../shell/history";
import { InputQueue, InputQueueOverflowError } from "../terminal/InputQueue";
import { MAX_INPUT_CHUNK_BYTES } from "../terminal/bytes";
import {
  VimInputStateMachine,
  type VimInputResult,
  type VimLineSnapshot,
} from "../vim";

const KIBIBYTE = 1_024;

describe("frontend performance budgets", () => {
  it(
    "batches a two-megabyte input burst without losing order or growing chunks",
    { timeout: 10_000 },
    async () => {
      const burstBytes = 2 * KIBIBYTE * KIBIBYTE;
      const enqueueBytes = 4 * KIBIBYTE;
      const source = Uint8Array.from(
        { length: burstBytes },
        (_, index) => index % 251,
      );
      let releaseFirstWrite: () => void = () => undefined;
      const firstWrite = new Promise<void>((resolve) => {
        releaseFirstWrite = resolve;
      });
      const writes: Uint8Array[] = [];
      const onError = vi.fn();
      const queue = new InputQueue(
        async (data) => {
          writes.push(data.slice());
          if (writes.length === 1) {
            await firstWrite;
          }
        },
        onError,
        { maxPendingBytes: burstBytes },
      );

      const started = performance.now();
      for (let offset = 0; offset < source.length; offset += enqueueBytes) {
        queue.enqueue(source.slice(offset, offset + enqueueBytes));
      }
      const enqueueDuration = performance.now() - started;

      expect(writes).toHaveLength(1);
      expectWithinBudget("input burst enqueue", enqueueDuration, 1_500);

      releaseFirstWrite();
      await queue.whenIdle();
      const totalDuration = performance.now() - started;

      const receivedBytes = writes.reduce(
        (total, chunk) => total + chunk.byteLength,
        0,
      );
      const minimumWrites = Math.ceil(burstBytes / MAX_INPUT_CHUNK_BYTES);
      const maximumWrites =
        1 + Math.ceil((burstBytes - enqueueBytes) / MAX_INPUT_CHUNK_BYTES);
      expect(receivedBytes).toBe(burstBytes);
      expect(writes.length).toBeGreaterThanOrEqual(minimumWrites);
      expect(writes.length).toBeLessThanOrEqual(maximumWrites);
      expect(
        writes.every((chunk) => chunk.byteLength <= MAX_INPUT_CHUNK_BYTES),
      ).toBe(true);
      expect(orderedByteHash(writes)).toBe(orderedByteHash([source]));
      expect(onError).not.toHaveBeenCalled();
      expectWithinBudget("input burst drain", totalDuration, 2_500);
    },
  );

  it("rejects an oversized pending-input allocation before invoking the writer", () => {
    const maximumBytes = 1 * KIBIBYTE * KIBIBYTE;
    const writer = vi.fn().mockResolvedValue(undefined);
    const onError = vi.fn();
    const queue = new InputQueue(writer, onError, {
      maxPendingBytes: maximumBytes,
    });

    const started = performance.now();
    queue.enqueue(new Uint8Array(maximumBytes + 1));
    const duration = performance.now() - started;

    expect(writer).not.toHaveBeenCalled();
    expect(onError).toHaveBeenCalledOnce();
    expect(onError).toHaveBeenCalledWith(expect.any(InputQueueOverflowError));
    expectWithinBudget("pending-input rejection", duration, 250);
  });

  it(
    "keeps Vim motions and edits responsive at the tracked command-line limit",
    { timeout: 10_000 },
    () => {
      const ascii = "alpha beta /tmp/value ++ ".repeat(1_200);
      const unicodeSuffix = "👩🏽‍💻";
      const line = ascii + unicodeSuffix;
      expect(new TextEncoder().encode(line).byteLength).toBeLessThanOrEqual(
        32 * KIBIBYTE,
      );

      const machine = new VimInputStateMachine({
        line,
        cursor: line.length,
      });
      const started = performance.now();
      let current = handledEdit(
        machine.handleKey(
          { key: "Escape" },
          { line, cursor: line.length },
        ),
      );

      for (let iteration = 0; iteration < 4; iteration += 1) {
        for (const command of ["0", "w", "e", "$", "b", "h", "l"]) {
          current = handledEdit(
            machine.handleKey({ key: command }, current),
          );
        }
      }

      current = handledEdit(machine.handleKey({ key: "$" }, current));
      current = handledEdit(machine.handleKey({ key: "x" }, current));
      expect(current.line).toBe(ascii);
      current = handledEdit(machine.handleKey({ key: "u" }, current));
      expect(current.line).toBe(line);

      current = handledEdit(machine.handleKey({ key: "0" }, current));
      current = handledEdit(machine.handleKey({ key: "D" }, current));
      expect(current).toEqual({ line: "", cursor: 0 });
      current = handledEdit(machine.handleKey({ key: "u" }, current));
      expect(current.line).toBe(line);

      current = handledEdit(machine.handleKey({ key: "d" }, current));
      current = handledEdit(machine.handleKey({ key: "d" }, current));
      expect(current).toEqual({ line: "", cursor: 0 });
      const duration = performance.now() - started;

      expectWithinBudget("large-line Vim command sequence", duration, 4_000);
    },
  );

  it(
    "ranks bounded history and builds completion menus within interactive budgets",
    { timeout: 10_000 },
    () => {
      const history = boundedHistory();
      const candidates = boundedCompletionCandidates();
      const longLine = `${"echo argument ".repeat(300)}cat src/ter`;
      const started = performance.now();
      let bestCommand = "";

      for (let iteration = 0; iteration < 40; iteration += 1) {
        for (const prefix of ["g", "gi", "git", "git ", "git command-"]) {
          bestCommand = rankAutosuggestions(prefix, history, { limit: 5 })[0]
            ?.command ?? "";
        }
      }

      let choiceCount = 0;
      for (let iteration = 0; iteration < 40; iteration += 1) {
        const request = completionRequestForLine(
          longLine,
          longLine.length,
          "/workspace",
        );
        expect(request).toEqual({
          kind: "path",
          prefix: "src/ter",
          cwd: "/workspace",
        });
        choiceCount = buildCompletionChoices(
          longLine,
          longLine.length,
          candidates,
        ).length;
      }
      const duration = performance.now() - started;

      expect(bestCommand).toBe("git command-0998");
      expect(choiceCount).toBe(20);
      expectWithinBudget(
        "history ranking and completion hot paths",
        duration,
        3_000,
      );
    },
  );
});

function handledEdit(result: VimInputResult): VimLineSnapshot {
  expect(result.kind).toBe("handled");
  if (result.kind !== "handled") {
    throw new Error(`Expected a handled Vim edit, received ${result.kind}`);
  }
  return result.edit;
}

function boundedHistory(): HistoryEntry[] {
  return Array.from({ length: 1_000 }, (_, index) => ({
    command:
      index % 2 === 0
        ? `git command-${index.toString().padStart(4, "0")}`
        : `npm command-${index.toString().padStart(4, "0")}`,
    frequency: (index % 11) + 1,
    lastUsedSequence: index,
  }));
}

function boundedCompletionCandidates(): NativeCompletionCandidate[] {
  return Array.from({ length: 100 }, (_, index) => ({
    value: `src/terminal-file-${index.toString().padStart(3, "0")}.ts`,
    display: `terminal-file-${index.toString().padStart(3, "0")}.ts`,
    kind: "file" as const,
  }));
}

function orderedByteHash(chunks: readonly Uint8Array[]): number {
  let hash = 2_166_136_261;
  for (const chunk of chunks) {
    for (const byte of chunk) {
      hash = Math.imul(hash ^ byte, 16_777_619) >>> 0;
    }
  }
  return hash;
}

function expectWithinBudget(
  label: string,
  durationMilliseconds: number,
  budgetMilliseconds: number,
): void {
  expect(
    durationMilliseconds,
    `${label} took ${durationMilliseconds.toFixed(1)}ms; budget is ${budgetMilliseconds}ms`,
  ).toBeLessThan(budgetMilliseconds);
}

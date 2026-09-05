import { describe, expect, it, vi } from "vitest";
import { InputQueue, InputQueueOverflowError } from "./InputQueue";

describe("InputQueue", () => {
  it("serializes writes and preserves their byte order", async () => {
    const writes: number[][] = [];
    let concurrent = 0;
    let maximumConcurrent = 0;
    const queue = new InputQueue(async (data) => {
      concurrent += 1;
      maximumConcurrent = Math.max(maximumConcurrent, concurrent);
      await Promise.resolve();
      writes.push(Array.from(data));
      concurrent -= 1;
    }, vi.fn());

    queue.enqueue(new Uint8Array([1, 2]));
    queue.enqueue(new Uint8Array([3]));
    queue.enqueue(new Uint8Array([4, 5]));
    await queue.whenIdle();

    expect(writes.flat()).toEqual([1, 2, 3, 4, 5]);
    expect(maximumConcurrent).toBe(1);
  });

  it("reports a failed write and drops queued input", async () => {
    const error = new Error("disconnected");
    const onError = vi.fn();
    const writer = vi.fn().mockRejectedValue(error);
    const queue = new InputQueue(writer, onError);

    queue.enqueue(new Uint8Array([1]));
    queue.enqueue(new Uint8Array([2]));
    await queue.whenIdle();

    expect(writer).toHaveBeenCalledTimes(1);
    expect(onError).toHaveBeenCalledWith(error);
  });

  it("retries a backpressured chunk without losing or reordering bytes", async () => {
    const writes: number[][] = [];
    const onError = vi.fn();
    let attempts = 0;
    const queue = new InputQueue(
      async (data) => {
        attempts += 1;
        if (attempts === 1) {
          throw { code: "input_backpressure" };
        }
        writes.push(Array.from(data));
      },
      onError,
      {
        isRetryable: (error) =>
          typeof error === "object" &&
          error !== null &&
          "code" in error &&
          error.code === "input_backpressure",
        waitBeforeRetry: async () => undefined,
      },
    );

    queue.enqueue(new Uint8Array([1, 2, 3]));
    queue.enqueue(new Uint8Array([4, 5]));
    await queue.whenIdle();

    expect(attempts).toBe(3);
    expect(writes.flat()).toEqual([1, 2, 3, 4, 5]);
    expect(onError).not.toHaveBeenCalled();
  });

  it("fails explicitly before pending input can grow without bound", async () => {
    let releaseWrite: () => void = () => undefined;
    const writer = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          releaseWrite = resolve;
        }),
    );
    const onError = vi.fn();
    const queue = new InputQueue(writer, onError, { maxPendingBytes: 3 });

    queue.enqueue(new Uint8Array([1, 2]));
    queue.enqueue(new Uint8Array([3, 4]));
    await queue.whenIdle();

    expect(onError).toHaveBeenCalledWith(expect.any(InputQueueOverflowError));
    queue.enqueue(new Uint8Array([5]));
    expect(writer).toHaveBeenCalledTimes(1);
    releaseWrite();
  });

  it("ignores input after disposal", async () => {
    const writer = vi.fn().mockResolvedValue(undefined);
    const queue = new InputQueue(writer, vi.fn());
    queue.dispose();
    queue.enqueue(new Uint8Array([1]));
    await queue.whenIdle();
    expect(writer).not.toHaveBeenCalled();
  });
});

import {
  MAX_INPUT_CHUNK_BYTES,
  chunkBytes,
  concatBytes,
} from "./bytes";

type InputWriter = (data: Uint8Array) => Promise<void>;

const DEFAULT_MAX_PENDING_BYTES = 4 * 1024 * 1024;
const INITIAL_RETRY_DELAY_MILLISECONDS = 4;
const MAX_RETRY_DELAY_MILLISECONDS = 100;

export interface InputQueueOptions {
  maxPendingBytes?: number;
  isRetryable?: (error: unknown) => boolean;
  waitBeforeRetry?: (delayMilliseconds: number) => Promise<void>;
}

export class InputQueueOverflowError extends Error {
  constructor(maximumBytes: number) {
    super(`Terminal input exceeds the ${maximumBytes}-byte pending-input limit`);
    this.name = "InputQueueOverflowError";
  }
}

export class InputQueue {
  private readonly queued: Uint8Array[] = [];
  private readonly maxPendingBytes: number;
  private readonly isRetryable: (error: unknown) => boolean;
  private readonly waitBeforeRetry: (
    delayMilliseconds: number,
  ) => Promise<void>;
  private draining = false;
  private disposed = false;
  private pendingBytes = 0;
  private idleWaiters: Array<() => void> = [];

  constructor(
    private readonly writer: InputWriter,
    private readonly onError: (error: unknown) => void,
    options: InputQueueOptions = {},
  ) {
    this.maxPendingBytes =
      options.maxPendingBytes ?? DEFAULT_MAX_PENDING_BYTES;
    this.isRetryable = options.isRetryable ?? (() => false);
    this.waitBeforeRetry = options.waitBeforeRetry ?? wait;

    if (!Number.isSafeInteger(this.maxPendingBytes) || this.maxPendingBytes < 1) {
      throw new RangeError("maxPendingBytes must be a positive safe integer");
    }
  }

  enqueue(data: Uint8Array): void {
    if (this.disposed || data.byteLength === 0) {
      return;
    }

    if (this.pendingBytes + data.byteLength > this.maxPendingBytes) {
      this.fail(new InputQueueOverflowError(this.maxPendingBytes));
      return;
    }

    this.pendingBytes += data.byteLength;

    for (const chunk of chunkBytes(data)) {
      const lastIndex = this.queued.length - 1;
      const last = this.queued[lastIndex];
      if (
        last &&
        last.byteLength + chunk.byteLength <= MAX_INPUT_CHUNK_BYTES
      ) {
        this.queued[lastIndex] = concatBytes(last, chunk);
      } else {
        this.queued.push(chunk);
      }
    }

    void this.drain();
  }

  dispose(): void {
    this.disposed = true;
    this.queued.length = 0;
    this.pendingBytes = 0;
    this.resolveIdleWaiters();
  }

  whenIdle(): Promise<void> {
    if (this.disposed || (!this.draining && this.queued.length === 0)) {
      return Promise.resolve();
    }

    return new Promise((resolve) => {
      this.idleWaiters.push(resolve);
    });
  }

  private async drain(): Promise<void> {
    if (this.draining || this.disposed) {
      return;
    }

    this.draining = true;
    try {
      while (!this.disposed) {
        const next = this.queued.shift();
        if (!next) {
          break;
        }

        let retryDelay = INITIAL_RETRY_DELAY_MILLISECONDS;
        while (!this.disposed) {
          try {
            await this.writer(next);
            this.pendingBytes = Math.max(
              0,
              this.pendingBytes - next.byteLength,
            );
            break;
          } catch (error) {
            if (!this.isRetryable(error)) {
              throw error;
            }
            await this.waitBeforeRetry(retryDelay);
            retryDelay = Math.min(
              retryDelay * 2,
              MAX_RETRY_DELAY_MILLISECONDS,
            );
          }
        }
      }
    } catch (error) {
      this.fail(error);
    } finally {
      this.draining = false;
      if (this.queued.length === 0 || this.disposed) {
        this.resolveIdleWaiters();
      } else {
        void this.drain();
      }
    }
  }

  private resolveIdleWaiters(): void {
    for (const resolve of this.idleWaiters.splice(0)) {
      resolve();
    }
  }

  private fail(error: unknown): void {
    if (this.disposed) {
      return;
    }
    this.disposed = true;
    this.queued.length = 0;
    this.pendingBytes = 0;
    this.resolveIdleWaiters();
    this.onError(error);
  }
}

function wait(delayMilliseconds: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, delayMilliseconds));
}

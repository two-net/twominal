export const DEFAULT_HISTORY_CAPACITY = 1_000;
export const MAX_HISTORY_CAPACITY = 100_000;
export const MAX_HISTORY_COMMAND_BYTES = 4_096;

export interface HistoryEntry {
  readonly command: string;
  readonly frequency: number;
  readonly lastUsedSequence: number;
}

export interface HistoryRecordOptions {
  /**
   * Callers should set this when command input came from a private/sensitive
   * interaction. The history layer intentionally does not guess based on words
   * such as "password", which would create unpredictable false positives.
   */
  readonly private?: boolean;
}

export interface HistoryStoreOptions {
  readonly capacity?: number;
}

export interface HistorySource {
  /** Returns entries in chronological order, oldest first. */
  entries(): readonly HistoryEntry[];
}

/**
 * Returns a safe, canonical history value or null when a command must not be
 * retained. Leading whitespace follows the common shell "private command"
 * convention; trailing whitespace is not significant and is removed.
 */
export function normalizeHistoryCommand(
  value: string,
  options: HistoryRecordOptions = {},
): string | null {
  if (
    options.private === true ||
    value.length === 0 ||
    new TextEncoder().encode(value).byteLength > MAX_HISTORY_COMMAND_BYTES ||
    /^\s/u.test(value) ||
    containsUnsafeHistoryCharacter(value)
  ) {
    return null;
  }

  const normalized = value.trimEnd();
  return normalized.length > 0 ? normalized : null;
}

/** A bounded, distinct-command history ordered from least to most recent. */
export class HistoryStore implements HistorySource {
  readonly capacity: number;

  private readonly commands: HistoryEntry[] = [];
  private sequence = 0;

  constructor(options: HistoryStoreOptions = {}) {
    this.capacity = validateCapacity(
      options.capacity ?? DEFAULT_HISTORY_CAPACITY,
    );
  }

  get size(): number {
    return this.commands.length;
  }

  /**
   * Records a command and returns true when it was accepted. Repeated commands
   * are promoted to most-recent rather than consuming an additional slot.
   */
  record(value: string, options: HistoryRecordOptions = {}): boolean {
    const command = normalizeHistoryCommand(value, options);
    if (command === null) {
      return false;
    }

    const existingIndex = this.commands.findIndex(
      (entry) => entry.command === command,
    );
    const existing =
      existingIndex === -1
        ? undefined
        : this.commands.splice(existingIndex, 1)[0];

    this.sequence += 1;
    this.commands.push({
      command,
      frequency: (existing?.frequency ?? 0) + 1,
      lastUsedSequence: this.sequence,
    });

    if (this.commands.length > this.capacity) {
      this.commands.shift();
    }
    return true;
  }

  entries(): readonly HistoryEntry[] {
    return this.commands.map(copyEntry);
  }

  /** Returns up to limit entries, newest first. */
  recent(limit = this.capacity): readonly HistoryEntry[] {
    const safeLimit = normalizeLimit(limit, this.capacity);
    if (safeLimit === 0) {
      return [];
    }
    return this.commands.slice(-safeLimit).reverse().map(copyEntry);
  }

  clear(): void {
    this.commands.length = 0;
    this.sequence = 0;
  }
}

/**
 * Prefix-filtered history navigation with shell-like draft restoration.
 * Call reset() when the user edits a recalled history value.
 */
export class HistoryNavigator {
  private draft = "";
  private matches: readonly string[] = [];
  private matchIndex = -1;

  constructor(private readonly source: HistorySource) {}

  get isNavigating(): boolean {
    return this.matchIndex >= 0;
  }

  previous(currentDraft: string): string {
    if (!this.isNavigating) {
      this.begin(currentDraft);
    }

    if (this.matches.length === 0) {
      return this.draft;
    }

    this.matchIndex = Math.min(this.matchIndex + 1, this.matches.length - 1);
    return this.matches[this.matchIndex];
  }

  next(currentDraft: string): string {
    if (!this.isNavigating) {
      return currentDraft;
    }

    if (this.matchIndex > 0) {
      this.matchIndex -= 1;
      return this.matches[this.matchIndex];
    }

    const draft = this.draft;
    this.reset();
    return draft;
  }

  reset(): void {
    this.draft = "";
    this.matches = [];
    this.matchIndex = -1;
  }

  private begin(draft: string): void {
    this.draft = draft;
    this.matches = this.source
      .entries()
      .filter((entry) => entry.command.startsWith(draft))
      .reverse()
      .map((entry) => entry.command);
    this.matchIndex = -1;
  }
}

function validateCapacity(value: number): number {
  if (
    !Number.isSafeInteger(value) ||
    value < 1 ||
    value > MAX_HISTORY_CAPACITY
  ) {
    throw new RangeError(
      `History capacity must be an integer from 1 to ${MAX_HISTORY_CAPACITY}.`,
    );
  }
  return value;
}

function normalizeLimit(value: number, maximum: number): number {
  if (!Number.isFinite(value) || value <= 0) {
    return 0;
  }
  return Math.min(Math.trunc(value), maximum);
}

function copyEntry(entry: HistoryEntry): HistoryEntry {
  return { ...entry };
}

function containsUnsafeHistoryCharacter(value: string): boolean {
  for (const character of value) {
    const codePoint = character.codePointAt(0) ?? 0;
    if (
      codePoint <= 0x1f ||
      (codePoint >= 0x7f && codePoint <= 0x9f) ||
      codePoint === 0x061c ||
      codePoint === 0x200e ||
      codePoint === 0x200f ||
      (codePoint >= 0x202a && codePoint <= 0x202e) ||
      (codePoint >= 0x2066 && codePoint <= 0x206f) ||
      codePoint === 0xfeff
    ) {
      return true;
    }
  }
  return false;
}

import type { HistoryEntry } from "./history";

export const DEFAULT_AUTOSUGGESTION_LIMIT = 5;
export const MAX_AUTOSUGGESTION_LIMIT = 50;

export interface Autosuggestion {
  readonly command: string;
  readonly suffix: string;
  readonly frequency: number;
  readonly lastUsedSequence: number;
}

export interface AutosuggestionOptions {
  readonly limit?: number;
}

/**
 * Ranks exact, case-sensitive prefix matches. More recently used commands win;
 * frequency and then code-unit ordering provide deterministic tie-breakers.
 */
export function rankAutosuggestions(
  prefix: string,
  entries: readonly HistoryEntry[],
  options: AutosuggestionOptions = {},
): readonly Autosuggestion[] {
  if (prefix.length === 0 || containsControlCharacter(prefix)) {
    return [];
  }

  const limit = normalizeSuggestionLimit(
    options.limit ?? DEFAULT_AUTOSUGGESTION_LIMIT,
  );
  if (limit === 0) {
    return [];
  }

  return entries
    .filter(
      (entry) =>
        entry.command.length > prefix.length &&
        entry.command.startsWith(prefix),
    )
    .sort(compareHistoryCandidates)
    .slice(0, limit)
    .map((entry) => ({
      command: entry.command,
      suffix: entry.command.slice(prefix.length),
      frequency: entry.frequency,
      lastUsedSequence: entry.lastUsedSequence,
    }));
}

export function bestAutosuggestion(
  prefix: string,
  entries: readonly HistoryEntry[],
): Autosuggestion | null {
  return rankAutosuggestions(prefix, entries, { limit: 1 })[0] ?? null;
}

function compareHistoryCandidates(
  left: HistoryEntry,
  right: HistoryEntry,
): number {
  const recency = right.lastUsedSequence - left.lastUsedSequence;
  if (recency !== 0) {
    return recency;
  }

  const frequency = right.frequency - left.frequency;
  if (frequency !== 0) {
    return frequency;
  }

  if (left.command < right.command) {
    return -1;
  }
  return left.command > right.command ? 1 : 0;
}

function normalizeSuggestionLimit(value: number): number {
  if (!Number.isFinite(value) || value <= 0) {
    return 0;
  }
  return Math.min(Math.trunc(value), MAX_AUTOSUGGESTION_LIMIT);
}

function containsControlCharacter(value: string): boolean {
  for (const character of value) {
    const codePoint = character.codePointAt(0) ?? 0;
    if (
      codePoint <= 0x1f ||
      (codePoint >= 0x7f && codePoint <= 0x9f)
    ) {
      return true;
    }
  }
  return false;
}

import { useCallback, useEffect, useRef, useState } from "react";
import {
  TauriShellClient,
  normalizeShellError,
  type HistoryEntry,
  type ShellClient,
} from "../shell";

const defaultClient = new TauriShellClient();

export type ShellHistoryStatus = "loading" | "ready" | "saving" | "error";

export interface ShellHistoryState {
  readonly entries: readonly HistoryEntry[];
  readonly status: ShellHistoryStatus;
  readonly errorMessage?: string;
  readonly recordCommand: (command: string) => void;
  readonly clearHistory: () => Promise<void>;
  readonly retryLoad: () => void;
}

export function useShellHistory(
  client: ShellClient = defaultClient,
): ShellHistoryState {
  const [entries, setEntries] = useState<readonly HistoryEntry[]>([]);
  const [status, setStatus] = useState<ShellHistoryStatus>("loading");
  const [errorMessage, setErrorMessage] = useState<string>();
  const [loadRevision, setLoadRevision] = useState(0);
  const mountedRef = useRef(true);
  const generationRef = useRef(0);
  const sequenceRef = useRef(0);
  const operationQueueRef = useRef<Promise<unknown>>(Promise.resolve());

  useEffect(() => {
    mountedRef.current = true;
    const generation = ++generationRef.current;
    setStatus("loading");
    setErrorMessage(undefined);

    void client
      .loadHistory()
      .then((loaded) => {
        if (!mountedRef.current || generation !== generationRef.current) {
          return;
        }
        const normalized = loaded.map((entry, index) => ({
          ...entry,
          lastUsedSequence: index + 1,
        }));
        sequenceRef.current = normalized.length;
        setEntries(normalized);
        setStatus("ready");
      })
      .catch((error: unknown) => {
        if (!mountedRef.current || generation !== generationRef.current) {
          return;
        }
        const normalized = normalizeShellError(
          error,
          "Twominal could not load command history.",
        );
        setStatus("error");
        setErrorMessage(normalized.message);
      });

    return () => {
      mountedRef.current = false;
    };
  }, [client, loadRevision]);

  const recordCommand = useCallback(
    (command: string) => {
      const generation = generationRef.current;
      operationQueueRef.current = operationQueueRef.current
        .catch(() => undefined)
        .then(() => client.appendHistory(command));
      void operationQueueRef.current
        .then((result) => {
          if (
            !mountedRef.current ||
            generation !== generationRef.current ||
            result === null
          ) {
            return;
          }
          const entry = result as HistoryEntry;
          sequenceRef.current += 1;
          setEntries((current) => [
            ...current.filter((candidate) => candidate.command !== entry.command),
            { ...entry, lastUsedSequence: sequenceRef.current },
          ]);
          setStatus("ready");
          setErrorMessage(undefined);
        })
        .catch((error: unknown) => {
          if (!mountedRef.current || generation !== generationRef.current) {
            return;
          }
          const normalized = normalizeShellError(
            error,
            "Twominal could not save command history.",
          );
          setStatus("error");
          setErrorMessage(normalized.message);
        });
    },
    [client],
  );

  const clearHistory = useCallback(async () => {
    const generation = ++generationRef.current;
    setStatus("saving");
    setErrorMessage(undefined);
    operationQueueRef.current = operationQueueRef.current
      .catch(() => undefined)
      .then(() => client.clearHistory());
    try {
      await operationQueueRef.current;
      if (!mountedRef.current || generation !== generationRef.current) {
        return;
      }
      sequenceRef.current = 0;
      setEntries([]);
      setStatus("ready");
    } catch (error) {
      if (!mountedRef.current || generation !== generationRef.current) {
        return;
      }
      const normalized = normalizeShellError(
        error,
        "Twominal could not clear command history.",
      );
      setStatus("error");
      setErrorMessage(normalized.message);
      throw normalized;
    }
  }, [client]);

  const retryLoad = useCallback(() => {
    setLoadRevision((revision) => revision + 1);
  }, []);

  return {
    entries,
    status,
    errorMessage,
    recordCommand,
    clearHistory,
    retryLoad,
  };
}

import { invoke } from "@tauri-apps/api/core";
import type { AppConfig } from "./types";

const UNKNOWN_ERROR_CODE = "config_unavailable";

export interface ConfigClient {
  load(): Promise<AppConfig>;
  save(config: AppConfig): Promise<AppConfig>;
}

export class ConfigClientError extends Error {
  readonly code: string;

  constructor(code: string, message: string, cause?: unknown) {
    super(message, cause === undefined ? undefined : { cause });
    this.name = "ConfigClientError";
    this.code = code;
  }
}

export class TauriConfigClient implements ConfigClient {
  async load(): Promise<AppConfig> {
    try {
      return await invoke<AppConfig>("config_load");
    } catch (error) {
      throw normalizeConfigError(
        error,
        "Twominal could not load its settings.",
      );
    }
  }

  async save(config: AppConfig): Promise<AppConfig> {
    try {
      return await invoke<AppConfig>("config_save", { config });
    } catch (error) {
      throw normalizeConfigError(
        error,
        "Twominal could not save its settings.",
      );
    }
  }
}

export function normalizeConfigError(
  error: unknown,
  fallbackMessage: string,
): ConfigClientError {
  if (error instanceof ConfigClientError) {
    return error;
  }

  if (isRecord(error)) {
    const code =
      typeof error.code === "string" && error.code.length > 0
        ? error.code
        : UNKNOWN_ERROR_CODE;
    const message =
      typeof error.message === "string" && error.message.length > 0
        ? error.message
        : fallbackMessage;
    return new ConfigClientError(code, message, error);
  }

  if (error instanceof Error) {
    return new ConfigClientError(
      UNKNOWN_ERROR_CODE,
      error.message || fallbackMessage,
      error,
    );
  }

  if (typeof error === "string" && error.length > 0) {
    return new ConfigClientError(UNKNOWN_ERROR_CODE, error, error);
  }

  return new ConfigClientError(UNKNOWN_ERROR_CODE, fallbackMessage, error);
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

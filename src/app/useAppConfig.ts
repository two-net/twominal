import { useCallback, useEffect, useRef, useState } from "react";
import type { Dispatch, SetStateAction } from "react";
import {
  TauriConfigClient,
  normalizeConfigError,
  type ConfigClient,
} from "../config/configClient";
import {
  createDefaultAppConfig,
  type AppConfig,
} from "../config/types";

const SAVE_DEBOUNCE_MILLISECONDS = 250;
const defaultClient = new TauriConfigClient();

export type AppConfigSaveStatus = "idle" | "saving" | "saved" | "error";

export interface AppConfigState {
  config: AppConfig;
  setConfig: Dispatch<SetStateAction<AppConfig>>;
  ready: boolean;
  saveStatus: AppConfigSaveStatus;
  errorMessage?: string;
  retrySave: () => void;
}

export function useAppConfig(
  client: ConfigClient = defaultClient,
): AppConfigState {
  const [config, setConfig] = useState(createDefaultAppConfig);
  const [loaded, setLoaded] = useState(false);
  const [saveStatus, setSaveStatus] =
    useState<AppConfigSaveStatus>("idle");
  const [errorMessage, setErrorMessage] = useState<string>();
  const [saveRevision, setSaveRevision] = useState(0);
  const mountedRef = useRef(true);
  const configRef = useRef(config);
  const baselineRef = useRef<string | null>(null);
  const lastSavedRef = useRef<string | null>(null);
  const saveGenerationRef = useRef(0);
  const saveQueueRef = useRef<Promise<unknown>>(Promise.resolve());
  configRef.current = config;

  useEffect(() => {
    mountedRef.current = true;
    let active = true;

    void client
      .load()
      .then((loadedConfig) => {
        if (!active) {
          return;
        }
        const serialized = serializeConfig(loadedConfig);
        baselineRef.current = serialized;
        lastSavedRef.current = serialized;
        configRef.current = loadedConfig;
        setConfig(loadedConfig);
        setSaveStatus("saved");
        setErrorMessage(undefined);
      })
      .catch((error: unknown) => {
        if (!active) {
          return;
        }
        const normalized = normalizeConfigError(
          error,
          "Twominal could not load its settings.",
        );
        baselineRef.current = serializeConfig(configRef.current);
        setSaveStatus("error");
        setErrorMessage(normalized.message);
      })
      .finally(() => {
        if (active) {
          setLoaded(true);
        }
      });

    return () => {
      active = false;
      mountedRef.current = false;
    };
  }, [client]);

  useEffect(() => {
    if (!loaded) {
      return;
    }

    const serialized = serializeConfig(config);
    if (baselineRef.current === serialized) {
      return;
    }
    baselineRef.current = null;
    if (lastSavedRef.current === serialized) {
      setSaveStatus("saved");
      setErrorMessage(undefined);
      return;
    }

    const generation = ++saveGenerationRef.current;
    setSaveStatus("saving");
    setErrorMessage(undefined);
    const timer = window.setTimeout(() => {
      const requestedConfig = config;
      saveQueueRef.current = saveQueueRef.current
        .catch(() => undefined)
        .then(() => client.save(requestedConfig));

      void saveQueueRef.current
        .then((result) => {
          if (!mountedRef.current || generation !== saveGenerationRef.current) {
            return;
          }
          const savedConfig = result as AppConfig;
          const savedSerialized = serializeConfig(savedConfig);
          lastSavedRef.current = savedSerialized;
          setSaveStatus("saved");
          setErrorMessage(undefined);
          if (
            serializeConfig(configRef.current) === serialized &&
            savedSerialized !== serialized
          ) {
            configRef.current = savedConfig;
            setConfig(savedConfig);
          }
        })
        .catch((error: unknown) => {
          if (!mountedRef.current || generation !== saveGenerationRef.current) {
            return;
          }
          const normalized = normalizeConfigError(
            error,
            "Twominal could not save its settings.",
          );
          setSaveStatus("error");
          setErrorMessage(normalized.message);
        });
    }, SAVE_DEBOUNCE_MILLISECONDS);

    return () => window.clearTimeout(timer);
  }, [client, config, loaded, saveRevision]);

  const retrySave = useCallback(() => {
    baselineRef.current = null;
    lastSavedRef.current = null;
    setSaveRevision((revision) => revision + 1);
  }, []);

  return {
    config,
    setConfig,
    ready: loaded,
    saveStatus,
    errorMessage,
    retrySave,
  };
}

function serializeConfig(config: AppConfig): string {
  return JSON.stringify(config);
}

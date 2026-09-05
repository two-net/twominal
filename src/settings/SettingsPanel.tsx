import { useEffect, useId, useRef, useState } from "react";
import type { KeyboardEvent as ReactKeyboardEvent } from "react";
import {
  DEFAULT_APP_CONFIG,
  type AppConfig,
} from "../config/types";

export type SettingsSaveStatus = "idle" | "saving" | "saved" | "error";
export type HistorySettingsStatus = "loading" | "ready" | "saving" | "error";

interface SettingsPanelProps {
  config: AppConfig;
  open: boolean;
  saveStatus: SettingsSaveStatus;
  saveError?: string;
  onChange: (config: AppConfig) => void;
  onClose: () => void;
  onRetry: () => void;
  historyCount?: number;
  historyStatus?: HistorySettingsStatus;
  historyError?: string;
  onClearHistory?: () => void;
  onRetryHistory?: () => void;
}

export function SettingsPanel({
  config,
  open,
  saveStatus,
  saveError,
  onChange,
  onClose,
  onRetry,
  historyCount = 0,
  historyStatus = "ready",
  historyError,
  onClearHistory,
  onRetryHistory,
}: SettingsPanelProps) {
  const dialogRef = useRef<HTMLDivElement>(null);
  const closeButtonRef = useRef<HTMLButtonElement>(null);
  const closeCallbackRef = useRef(onClose);
  const [latitude, setLatitude] = useState(
    coordinateText(config.appearance.latitude),
  );
  const [longitude, setLongitude] = useState(
    coordinateText(config.appearance.longitude),
  );
  const [coordinateError, setCoordinateError] = useState("");
  const [confirmHistoryClear, setConfirmHistoryClear] = useState(false);
  closeCallbackRef.current = onClose;

  useEffect(() => {
    setLatitude(coordinateText(config.appearance.latitude));
    setLongitude(coordinateText(config.appearance.longitude));
  }, [config.appearance.latitude, config.appearance.longitude]);

  useEffect(() => {
    if (!open) {
      setConfirmHistoryClear(false);
      return;
    }

    const previouslyFocused = document.activeElement as HTMLElement | null;
    closeButtonRef.current?.focus();

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        closeCallbackRef.current();
        return;
      }
      if (event.key !== "Tab") {
        return;
      }

      const focusable = getFocusableElements(dialogRef.current);
      const first = focusable[0];
      const last = focusable.at(-1);
      if (!first || !last) {
        event.preventDefault();
        return;
      }
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    };

    document.addEventListener("keydown", handleKeyDown, true);
    return () => {
      document.removeEventListener("keydown", handleKeyDown, true);
      previouslyFocused?.focus();
    };
  }, [open]);

  if (!open) {
    return null;
  }

  const updateAppearance = (
    patch: Partial<AppConfig["appearance"]>,
  ): void => {
    onChange({
      ...config,
      appearance: { ...config.appearance, ...patch },
    });
  };

  const updateTerminal = (patch: Partial<AppConfig["terminal"]>): void => {
    onChange({
      ...config,
      terminal: { ...config.terminal, ...patch },
    });
  };

  const commitCoordinates = (): void => {
    const latitudeValue = optionalNumber(latitude);
    const longitudeValue = optionalNumber(longitude);
    if (latitudeValue === null && longitudeValue === null) {
      setCoordinateError("");
      updateAppearance({ latitude: null, longitude: null });
      return;
    }
    if (
      latitudeValue === null ||
      longitudeValue === null ||
      latitudeValue < -90 ||
      latitudeValue > 90 ||
      longitudeValue < -180 ||
      longitudeValue > 180
    ) {
      setCoordinateError(
        "Enter both coordinates: latitude −90 to 90 and longitude −180 to 180.",
      );
      return;
    }

    setCoordinateError("");
    updateAppearance({
      latitude: latitudeValue,
      longitude: longitudeValue,
    });
  };

  const restoreDefaults = (): void => {
    setCoordinateError("");
    onChange(cloneDefaultConfig());
  };

  return (
    <div
      className="settings-backdrop settings-modal-backdrop"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) {
          onClose();
        }
      }}
    >
      <div
        className="settings-dialog settings-modal"
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby="settings-title"
      >
        <header className="settings-header">
          <div className="settings-heading">
            <h1 id="settings-title">Twominal Settings</h1>
            <p>Changes apply to every terminal tab.</p>
          </div>
          <button
            ref={closeButtonRef}
            type="button"
            className="settings-close-button"
            aria-label="Close settings"
            onClick={onClose}
          >
            <span aria-hidden="true">×</span>
          </button>
        </header>

        <div className="settings-content settings-modal-body">
          <fieldset className="settings-section">
            <legend>Appearance</legend>
            <label className="settings-field">
              <span>Theme</span>
              <select
                value={config.appearance.mode}
                onChange={(event) =>
                  updateAppearance({
                    mode: event.currentTarget.value as AppConfig["appearance"]["mode"],
                  })
                }
              >
                <option value="system">Automatic — Follow System</option>
                <option value="sunSchedule">
                  Automatic — Sunset to Sunrise
                </option>
                <option value="light">Light</option>
                <option value="dark">Dark</option>
              </select>
            </label>

            {config.appearance.mode === "sunSchedule" ? (
              <div className="schedule-settings">
                <p id="schedule-help" className="settings-help">
                  Coordinates are optional and never requested automatically.
                  Without them, Twominal uses 6:00 PM to 6:00 AM local time.
                </p>
                <div className="coordinate-fields" aria-describedby="schedule-help">
                  <label className="settings-field">
                    <span>Latitude</span>
                    <input
                      type="number"
                      min="-90"
                      max="90"
                      step="0.0001"
                      inputMode="decimal"
                      value={latitude}
                      aria-invalid={Boolean(coordinateError)}
                      onChange={(event) => setLatitude(event.currentTarget.value)}
                      onBlur={commitCoordinates}
                    />
                  </label>
                  <label className="settings-field">
                    <span>Longitude</span>
                    <input
                      type="number"
                      min="-180"
                      max="180"
                      step="0.0001"
                      inputMode="decimal"
                      value={longitude}
                      aria-invalid={Boolean(coordinateError)}
                      onChange={(event) => setLongitude(event.currentTarget.value)}
                      onBlur={commitCoordinates}
                    />
                  </label>
                </div>
                {coordinateError ? (
                  <p className="settings-validation" role="alert">
                    {coordinateError}
                  </p>
                ) : null}
                {config.appearance.latitude !== null ? (
                  <button
                    type="button"
                    className="settings-secondary-button"
                    onClick={() => {
                      setLatitude("");
                      setLongitude("");
                      setCoordinateError("");
                      updateAppearance({ latitude: null, longitude: null });
                    }}
                  >
                    Clear coordinates
                  </button>
                ) : null}
              </div>
            ) : null}
          </fieldset>

          <fieldset className="settings-section">
            <legend>Font</legend>
            <TextSetting
              label="Font family"
              value={config.terminal.fontFamily}
              onCommit={(fontFamily) => updateTerminal({ fontFamily })}
            />
            <div className="settings-grid settings-range-grid">
              <RangeSetting
                label="Font size"
                value={config.terminal.fontSize}
                min={8}
                max={40}
                step={0.5}
                unit="px"
                onChange={(fontSize) => updateTerminal({ fontSize })}
              />
              <RangeSetting
                label="Line height"
                value={config.terminal.lineHeight}
                min={1}
                max={2}
                step={0.05}
                onChange={(lineHeight) => updateTerminal({ lineHeight })}
              />
              <RangeSetting
                label="Letter spacing"
                value={config.terminal.letterSpacing}
                min={-2}
                max={5}
                step={0.1}
                unit="px"
                onChange={(letterSpacing) =>
                  updateTerminal({ letterSpacing })
                }
              />
              <label className="settings-field">
                <span>Weight</span>
                <select
                  value={config.terminal.fontWeight}
                  onChange={(event) =>
                    updateTerminal({
                      fontWeight: Number(event.currentTarget.value),
                    })
                  }
                >
                  {FONT_WEIGHTS.map((weight) => (
                    <option value={weight} key={weight}>
                      {weight}
                    </option>
                  ))}
                </select>
              </label>
            </div>
            <ToggleSetting
              label="Programming ligatures"
              description="Join common coding sequences when the selected font supports them."
              checked={config.terminal.fontLigatures}
              onChange={(fontLigatures) => updateTerminal({ fontLigatures })}
            />
          </fieldset>

          <fieldset className="settings-section">
            <legend>Shell experience</legend>
            <p className="settings-help">
              Autosuggestions and completions activate only at verified shell
              prompts. Input is passed through unchanged while commands and
              terminal programs run.
            </p>
            <div className="settings-history-row">
              <div>
                <strong>Command history</strong>
                <span>
                  {historyCount === 1
                    ? "1 saved command"
                    : `${historyCount} saved commands`}
                </span>
              </div>
              <button
                type="button"
                className="settings-secondary-button"
                disabled={
                  (historyCount === 0 && historyStatus !== "error") ||
                  historyStatus === "loading" ||
                  historyStatus === "saving"
                }
                onClick={() => {
                  if (!confirmHistoryClear) {
                    setConfirmHistoryClear(true);
                    return;
                  }
                  setConfirmHistoryClear(false);
                  onClearHistory?.();
                }}
              >
                {confirmHistoryClear
                  ? "Confirm clear history"
                  : "Clear history"}
              </button>
            </div>
            {confirmHistoryClear ? (
              <p className="settings-validation" role="status">
                This permanently removes Twominal’s saved command history.
              </p>
            ) : null}
            {historyStatus === "error" ? (
              <div className="settings-inline-error" role="alert">
                <span>{historyError ?? "Command history is unavailable."}</span>
                <button
                  type="button"
                  className="settings-secondary-button"
                  onClick={onRetryHistory}
                >
                  Retry
                </button>
              </div>
            ) : null}
          </fieldset>

          <fieldset className="settings-section">
            <legend>Keyboard</legend>
            <ToggleSetting
              label="Vim-style command editing"
              description="Use Insert and Normal modes at verified shell prompts. Terminal programs always receive input directly."
              checked={config.vimMode}
              onChange={(vimMode) => onChange({ ...config, vimMode })}
            />
          </fieldset>

          <fieldset className="settings-section">
            <legend>Motion</legend>
            <ToggleSetting
              label="Interface animations"
              description="Reduced-motion system preferences always take priority."
              checked={config.animations}
              onChange={(animations) => onChange({ ...config, animations })}
            />
          </fieldset>
        </div>

        <footer className="settings-footer">
          <button
            type="button"
            className="settings-secondary-button"
            onClick={restoreDefaults}
          >
            Restore defaults
          </button>
          <span
            className={`settings-save-status is-${saveStatus}`}
            role={saveStatus === "error" ? "alert" : "status"}
            title={saveError}
          >
            {saveStatusLabel(saveStatus)}
          </span>
          {saveStatus === "error" ? (
            <button
              type="button"
              className="settings-secondary-button"
              onClick={onRetry}
            >
              Retry
            </button>
          ) : null}
        </footer>
      </div>
    </div>
  );
}

interface RangeSettingProps {
  label: string;
  value: number;
  min: number;
  max: number;
  step: number;
  unit?: string;
  onChange: (value: number) => void;
}

function RangeSetting({
  label,
  value,
  min,
  max,
  step,
  unit,
  onChange,
}: RangeSettingProps) {
  const id = useId();
  const displayValue = unit ? `${value} ${unit}` : String(value);
  const accessibleValue = unit === "px" ? `${value} pixels` : displayValue;

  return (
    <div className="settings-field settings-range-setting">
      <div className="settings-range-heading">
        <label htmlFor={id}>{label}</label>
        <output
          className="settings-range-value"
          htmlFor={id}
          aria-live="polite"
        >
          {displayValue}
        </output>
      </div>
      <input
        id={id}
        type="range"
        value={value}
        min={min}
        max={max}
        step={step}
        aria-valuetext={accessibleValue}
        onChange={(event) => onChange(event.currentTarget.valueAsNumber)}
      />
    </div>
  );
}

function TextSetting({
  label,
  value,
  onCommit,
}: {
  label: string;
  value: string;
  onCommit: (value: string) => void;
}) {
  const [draft, setDraft] = useState(value);
  useEffect(() => setDraft(value), [value]);

  const commit = () => {
    const normalized = draft.trim();
    if (!normalized) {
      setDraft(value);
      return;
    }
    setDraft(normalized);
    onCommit(normalized);
  };

  return (
    <label className="settings-field">
      <span>{label}</span>
      <input
        type="text"
        value={draft}
        maxLength={256}
        spellCheck={false}
        autoCapitalize="none"
        onChange={(event) => setDraft(event.currentTarget.value)}
        onBlur={commit}
        onKeyDown={(event) => blurOnEnter(event)}
      />
    </label>
  );
}

function ToggleSetting({
  label,
  description,
  checked,
  onChange,
}: {
  label: string;
  description: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
}) {
  return (
    <label className="toggle-setting">
      <span className="settings-switch-copy">
        <strong>{label}</strong>
        <small>{description}</small>
      </span>
      <span className="settings-switch">
        <input
          className="settings-switch-input"
          type="checkbox"
          checked={checked}
          onChange={(event) => onChange(event.currentTarget.checked)}
        />
        <span className="settings-switch-slider" aria-hidden="true" />
      </span>
    </label>
  );
}

const FONT_WEIGHTS = [100, 200, 300, 400, 500, 600, 700, 800, 900];

function coordinateText(value: number | null): string {
  return value === null ? "" : String(value);
}

function optionalNumber(value: string): number | null {
  if (!value.trim()) {
    return null;
  }
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : null;
}

function blurOnEnter(event: ReactKeyboardEvent<HTMLInputElement>): void {
  if (event.key === "Enter") {
    event.currentTarget.blur();
  }
}

function getFocusableElements(container: HTMLElement | null): HTMLElement[] {
  if (!container) {
    return [];
  }
  return Array.from(
    container.querySelectorAll<HTMLElement>(
      "button:not([disabled]), input:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex='-1'])",
    ),
  ).filter((element) => !element.hidden);
}

function cloneDefaultConfig(): AppConfig {
  return {
    ...DEFAULT_APP_CONFIG,
    appearance: { ...DEFAULT_APP_CONFIG.appearance },
    terminal: { ...DEFAULT_APP_CONFIG.terminal },
  };
}

function saveStatusLabel(status: SettingsSaveStatus): string {
  switch (status) {
    case "idle":
      return "Stored locally";
    case "saving":
      return "Saving…";
    case "saved":
      return "Saved";
    case "error":
      return "Settings unavailable";
  }
}

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  DEFAULT_APP_CONFIG,
  type AppConfig,
} from "../config/types";
import { SettingsPanel } from "./SettingsPanel";

afterEach(cleanup);

describe("SettingsPanel", () => {
  it("is modal, focuses its close action, and closes with Escape", () => {
    const onClose = vi.fn();
    renderPanel({ onClose });

    expect(
      screen.getByRole("dialog", { name: "Twominal Settings" }),
    ).toHaveAttribute("aria-modal", "true");
    expect(screen.getByRole("button", { name: "Close settings" })).toHaveFocus();

    fireEvent.keyDown(document, { key: "Escape" });
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("closes from the backdrop but not from interactions inside the modal", () => {
    const onClose = vi.fn();
    const { container } = renderPanel({ onClose });
    const backdrop = container.querySelector(".settings-backdrop");
    const dialog = screen.getByRole("dialog", { name: "Twominal Settings" });

    expect(backdrop).not.toBeNull();
    fireEvent.mouseDown(dialog);
    expect(onClose).not.toHaveBeenCalled();
    fireEvent.mouseDown(backdrop!);
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("keeps keyboard focus inside the modal", () => {
    renderPanel();
    const close = screen.getByRole("button", { name: "Close settings" });
    const restore = screen.getByRole("button", { name: "Restore defaults" });

    close.focus();
    fireEvent.keyDown(document, { key: "Tab", shiftKey: true });
    expect(restore).toHaveFocus();

    fireEvent.keyDown(document, { key: "Tab" });
    expect(close).toHaveFocus();
  });

  it("emits appearance and font changes", () => {
    const onChange = vi.fn();
    renderPanel({ onChange });

    fireEvent.change(screen.getByRole("combobox", { name: "Theme" }), {
      target: { value: "dark" },
    });
    expect(onChange).toHaveBeenLastCalledWith({
      ...DEFAULT_APP_CONFIG,
      appearance: { ...DEFAULT_APP_CONFIG.appearance, mode: "dark" },
    });

    const fontFamily = screen.getByRole("textbox", { name: "Font family" });
    fireEvent.change(fontFamily, { target: { value: "JetBrains Mono" } });
    fireEvent.blur(fontFamily);
    expect(onChange).toHaveBeenLastCalledWith({
      ...DEFAULT_APP_CONFIG,
      terminal: {
        ...DEFAULT_APP_CONFIG.terminal,
        fontFamily: "JetBrains Mono",
      },
    });
  });

  it("applies range settings live with visible values and production limits", () => {
    const onChange = vi.fn();
    renderPanel({ onChange });

    const fontSize = screen.getByRole("slider", { name: "Font size" });
    expect(fontSize).toHaveAttribute("min", "8");
    expect(fontSize).toHaveAttribute("max", "40");
    expect(fontSize).toHaveAttribute("step", "0.5");
    expect(screen.getByText("14 px", { selector: "output" })).toBeVisible();
    fireEvent.change(fontSize, { target: { value: "18.5" } });
    expect(onChange).toHaveBeenLastCalledWith({
      ...DEFAULT_APP_CONFIG,
      terminal: { ...DEFAULT_APP_CONFIG.terminal, fontSize: 18.5 },
    });

    const lineHeight = screen.getByRole("slider", { name: "Line height" });
    expect(lineHeight).toHaveAttribute("min", "1");
    expect(lineHeight).toHaveAttribute("max", "2");
    expect(lineHeight).toHaveAttribute("step", "0.05");
    expect(screen.getByText("1.18", { selector: "output" })).toBeVisible();
    fireEvent.change(lineHeight, { target: { value: "1.45" } });
    expect(onChange).toHaveBeenLastCalledWith({
      ...DEFAULT_APP_CONFIG,
      terminal: { ...DEFAULT_APP_CONFIG.terminal, lineHeight: 1.45 },
    });

    const letterSpacing = screen.getByRole("slider", {
      name: "Letter spacing",
    });
    expect(letterSpacing).toHaveAttribute("min", "-2");
    expect(letterSpacing).toHaveAttribute("max", "5");
    expect(letterSpacing).toHaveAttribute("step", "0.1");
    expect(screen.getByText("0 px", { selector: "output" })).toBeVisible();
    fireEvent.change(letterSpacing, { target: { value: "-0.6" } });
    expect(onChange).toHaveBeenLastCalledWith({
      ...DEFAULT_APP_CONFIG,
      terminal: { ...DEFAULT_APP_CONFIG.terminal, letterSpacing: -0.6 },
    });
  });

  it("keeps coordinates optional and commits only a valid pair", () => {
    const onChange = vi.fn();
    const config = cloneConfig();
    config.appearance.mode = "sunSchedule";
    renderPanel({ config, onChange });

    const latitude = screen.getByRole("spinbutton", { name: "Latitude" });
    const longitude = screen.getByRole("spinbutton", { name: "Longitude" });
    fireEvent.change(latitude, { target: { value: "13.7563" } });
    fireEvent.blur(latitude);
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Enter both coordinates",
    );
    expect(onChange).not.toHaveBeenCalled();

    fireEvent.change(longitude, { target: { value: "100.5018" } });
    fireEvent.blur(longitude);
    expect(onChange).toHaveBeenCalledWith({
      ...config,
      appearance: {
        ...config.appearance,
        latitude: 13.7563,
        longitude: 100.5018,
      },
    });
  });

  it("updates ligatures, Vim editing, and motion and restores defaults", () => {
    const onChange = vi.fn();
    const config = cloneConfig();
    config.terminal.fontLigatures = false;
    config.animations = false;
    renderPanel({ config, onChange });

    fireEvent.click(
      screen.getByRole("checkbox", { name: /Programming ligatures/ }),
    );
    expect(onChange).toHaveBeenLastCalledWith({
      ...config,
      terminal: { ...config.terminal, fontLigatures: true },
    });

    fireEvent.click(
      screen.getByRole("checkbox", { name: /Vim-style command editing/ }),
    );
    expect(onChange).toHaveBeenLastCalledWith({
      ...config,
      vimMode: true,
    });

    fireEvent.click(
      screen.getByRole("checkbox", { name: /Interface animations/ }),
    );
    expect(onChange).toHaveBeenLastCalledWith({
      ...config,
      animations: true,
    });

    fireEvent.click(screen.getByRole("button", { name: "Restore defaults" }));
    expect(onChange).toHaveBeenLastCalledWith(DEFAULT_APP_CONFIG);
  });

  it("does not render while closed", () => {
    renderPanel({ open: false });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("requires confirmation before clearing persisted command history", () => {
    const onClearHistory = vi.fn();
    renderPanel({ historyCount: 4, onClearHistory });

    fireEvent.click(screen.getByRole("button", { name: "Clear history" }));
    expect(onClearHistory).not.toHaveBeenCalled();
    expect(screen.getByText(/permanently removes/)).toBeVisible();

    fireEvent.click(
      screen.getByRole("button", { name: "Confirm clear history" }),
    );
    expect(onClearHistory).toHaveBeenCalledOnce();
  });

  it("allows malformed or unreadable history to be cleared for recovery", () => {
    const onClearHistory = vi.fn();
    renderPanel({
      historyCount: 0,
      historyStatus: "error",
      historyError: "Command history is malformed.",
      onClearHistory,
    });

    const clear = screen.getByRole("button", { name: "Clear history" });
    expect(clear).toBeEnabled();
    fireEvent.click(clear);
    fireEvent.click(
      screen.getByRole("button", { name: "Confirm clear history" }),
    );
    expect(onClearHistory).toHaveBeenCalledOnce();
  });

  it("retries independent settings and history failures", () => {
    const onRetry = vi.fn();
    const onRetryHistory = vi.fn();
    renderPanel({
      saveStatus: "error",
      saveError: "Settings could not be saved.",
      historyStatus: "error",
      historyError: "Command history is unavailable.",
      onRetry,
      onRetryHistory,
    });

    const retryButtons = screen.getAllByRole("button", { name: "Retry" });
    fireEvent.click(retryButtons[0]);
    expect(onRetryHistory).toHaveBeenCalledOnce();
    fireEvent.click(retryButtons[1]);
    expect(onRetry).toHaveBeenCalledOnce();
    expect(screen.getByText("Settings unavailable")).toHaveAttribute(
      "title",
      "Settings could not be saved.",
    );
  });
});

function renderPanel(
  overrides: Partial<React.ComponentProps<typeof SettingsPanel>> = {},
) {
  return render(
    <SettingsPanel
      config={cloneConfig()}
      open
      saveStatus="saved"
      onChange={vi.fn()}
      onClose={vi.fn()}
      onRetry={vi.fn()}
      {...overrides}
    />,
  );
}

function cloneConfig(): AppConfig {
  return {
    ...DEFAULT_APP_CONFIG,
    appearance: { ...DEFAULT_APP_CONFIG.appearance },
    terminal: { ...DEFAULT_APP_CONFIG.terminal },
  };
}

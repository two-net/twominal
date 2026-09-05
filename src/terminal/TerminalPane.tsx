import { useEffect, useRef } from "react";
import type { TerminalConfig } from "../config/types";
import type { ResolvedAppearance } from "../theme";
import type { HistoryEntry, ShellExperienceStatus } from "../shell";
import { TerminalController } from "./TerminalController";
import type { TerminalViewState } from "./types";

interface TerminalPaneProps {
  appearance: ResolvedAppearance;
  active: boolean;
  terminalConfig: TerminalConfig;
  vimMode: boolean;
  restartKey: number;
  onStateChange: (state: TerminalViewState) => void;
  onTitleChange: (title: string) => void;
  history: readonly HistoryEntry[];
  onCommandAccepted: (command: string) => void;
  onShellExperienceChange: (status: ShellExperienceStatus) => void;
  transferToken?: string;
}

export function TerminalPane({
  appearance,
  active,
  terminalConfig,
  vimMode,
  restartKey,
  onStateChange,
  onTitleChange,
  history,
  onCommandAccepted,
  onShellExperienceChange,
  transferToken,
}: TerminalPaneProps) {
  const hostRef = useRef<HTMLDivElement>(null);
  const controllerRef = useRef<TerminalController | null>(null);
  const controllerRestartKeyRef = useRef<number | null>(null);
  const disposeTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const stateCallbackRef = useRef(onStateChange);
  const titleCallbackRef = useRef(onTitleChange);
  const commandCallbackRef = useRef(onCommandAccepted);
  const shellExperienceCallbackRef = useRef(onShellExperienceChange);
  stateCallbackRef.current = onStateChange;
  titleCallbackRef.current = onTitleChange;
  commandCallbackRef.current = onCommandAccepted;
  shellExperienceCallbackRef.current = onShellExperienceChange;

  useEffect(() => {
    const host = hostRef.current;
    if (!host) {
      return;
    }

    if (disposeTimerRef.current !== null) {
      clearTimeout(disposeTimerRef.current);
      disposeTimerRef.current = null;
    }

    const existing = controllerRef.current;
    if (existing && controllerRestartKeyRef.current === restartKey) {
      return () => scheduleControllerDisposal(existing);
    }
    if (existing) {
      controllerRef.current = null;
      controllerRestartKeyRef.current = null;
      void existing.dispose();
    }

    const controller = new TerminalController(host, {
      appearance,
      active,
      terminalConfig,
      vimMode,
      history,
      onStateChange: (state) => stateCallbackRef.current(state),
      onTitleChange: (title) => titleCallbackRef.current(title),
      onCommandAccepted: (command) => commandCallbackRef.current(command),
      onShellExperienceChange: (status) =>
        shellExperienceCallbackRef.current(status),
      transferToken,
    });
    controllerRef.current = controller;
    controllerRestartKeyRef.current = restartKey;
    void controller.start();

    return () => scheduleControllerDisposal(controller);

    function scheduleControllerDisposal(controllerToDispose: TerminalController) {
      disposeTimerRef.current = setTimeout(() => {
        disposeTimerRef.current = null;
        if (controllerRef.current === controllerToDispose) {
          controllerRef.current = null;
          controllerRestartKeyRef.current = null;
        }
        void controllerToDispose.dispose();
      }, 0);
    }
  }, [restartKey]);

  useEffect(() => {
    controllerRef.current?.applyAppearance(appearance);
  }, [appearance]);

  useEffect(() => {
    controllerRef.current?.applyTerminalConfig(terminalConfig);
  }, [terminalConfig]);

  useEffect(() => {
    controllerRef.current?.applyVimMode(vimMode);
  }, [vimMode]);

  useEffect(() => {
    controllerRef.current?.updateHistory(history);
  }, [history]);

  useEffect(() => {
    controllerRef.current?.setActive(active);
  }, [active]);

  return <div ref={hostRef} className="terminal-host" aria-label="Terminal" />;
}

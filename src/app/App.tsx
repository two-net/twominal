import {
  useCallback,
  useEffect,
  useMemo,
  useReducer,
  useRef,
  useState,
} from "react";
import type { AppearanceMode } from "../config/types";
import type { ShellExperienceStatus } from "../shell";
import { SettingsPanel } from "../settings/SettingsPanel";
import {
  TabStrip,
  type TabDragPayload,
} from "../tabs/TabStrip";
import { isApplePlatform, tabShortcutFor } from "../tabs/shortcuts";
import {
  createInitialTabWorkspace,
  MAX_TABS,
  tabWorkspaceReducer,
} from "../tabs/tabState";
import {
  cancelTerminalTransfer,
  prepareTerminalTransfer,
} from "../terminal/terminalClient";
import { TerminalPane } from "../terminal/TerminalPane";
import type { TerminalViewState } from "../terminal/types";
import { ThemeScheduler, resolveThemeAppearance } from "../theme";
import type { ResolvedAppearance } from "../theme";
import { SplashScreen } from "../ui/SplashScreen";
import { useAppConfig } from "./useAppConfig";
import { useShellHistory } from "./useShellHistory";
import {
  closeCurrentWindow,
  createTransferIdentity,
  createTransferWindow,
  getWindowContext,
  isCursorOutsideTwominalWindows,
  listenForIncomingTransfers,
  listenForTabTransferRequests,
  notifyTransferResult,
  requestTabTransfer,
  sendIncomingTransfer,
  waitForTransferResult,
  type IncomingTabTransfer,
  type TabTransferRequest,
} from "../windows/windowRuntime";

const TAB_DETACH_GRACE_MS = 150;
const TAB_DRAG_LIFETIME_MS = 60_000;
const MAX_TRACKED_TAB_DRAGS = 64;

type TabTransferDestination =
  | { type: "new" }
  | {
      type: "existing";
      targetWindowLabel: string;
      toIndex: number;
    };

export function App() {
  const windowContext = useMemo(getWindowContext, []);
  const {
    config,
    setConfig,
    ready: configReady,
    saveStatus,
    errorMessage: configError,
    retrySave,
  } = useAppConfig();
  const {
    entries: shellHistory,
    status: historyStatus,
    errorMessage: historyError,
    recordCommand,
    clearHistory,
    retryLoad: retryHistory,
  } = useShellHistory();
  const [workspace, dispatch] = useReducer(
    tabWorkspaceReducer,
    windowContext.bootstrap
      ? {
          ...windowContext.bootstrap,
          title: windowContext.bootstrap.title,
        }
      : null,
    createInitialTabWorkspace,
  );
  const [showSplash, setShowSplash] = useState(true);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [closingTabIds, setClosingTabIds] = useState<readonly string[]>([]);
  const [transferringTabId, setTransferringTabId] = useState<string | null>(
    null,
  );
  const [windowActionError, setWindowActionError] = useState<string | null>(
    null,
  );
  const [shellExperienceByTab, setShellExperienceByTab] = useState<
    Readonly<Record<string, ShellExperienceStatus>>
  >({});
  const closeTimersRef = useRef(new Map<string, number>());
  const detachTimersRef = useRef(new Map<string, number>());
  const windowActionPendingRef = useRef(false);
  const workspaceRef = useRef(workspace);
  const acceptedTransfersRef = useRef(new Set<string>());
  const handledDragIdsRef = useRef(new Set<string>());
  const localTabDragsRef = useRef(
    new Map<string, { tabId: string; startedAt: number }>(),
  );
  workspaceRef.current = workspace;
  const [appearance, setAppearance] = useState<ResolvedAppearance>(() =>
    resolveThemeAppearance(config.appearance, getSystemDarkPreference()),
  );
  const themeSchedulerRef = useRef<ThemeScheduler | null>(null);
  const activeTab = useMemo(
    () => workspace.tabs.find((tab) => tab.id === workspace.activeId) ?? null,
    [workspace.activeId, workspace.tabs],
  );

  useEffect(() => {
    const scheduler = new ThemeScheduler(setAppearance);
    themeSchedulerRef.current = scheduler;
    scheduler.start(config.appearance);
    return () => {
      scheduler.stop();
      themeSchedulerRef.current = null;
    };
  }, []);

  useEffect(() => {
    themeSchedulerRef.current?.update(config.appearance);
  }, [config.appearance]);

  useEffect(() => {
    if (!configReady) {
      setSettingsOpen(false);
    }
  }, [configReady]);

  useEffect(() => {
    const openSettingsShortcut = (event: KeyboardEvent) => {
      if (
        !event.isComposing &&
        !event.repeat &&
        event.code === "Comma" &&
        !event.altKey &&
        !event.shiftKey &&
        (isApplePlatform(`${navigator.platform} ${navigator.userAgent}`)
          ? event.metaKey && !event.ctrlKey
          : event.ctrlKey && !event.metaKey)
      ) {
        event.preventDefault();
        event.stopPropagation();
        if (configReady) {
          setSettingsOpen(true);
        }
      }
    };
    window.addEventListener("keydown", openSettingsShortcut, { capture: true });
    return () =>
      window.removeEventListener("keydown", openSettingsShortcut, {
        capture: true,
      });
  }, [configReady]);

  const closeSettings = useCallback(() => {
    setSettingsOpen(false);
  }, []);

  useEffect(() => {
    document.documentElement.dataset.theme = appearance;
    document.documentElement.dataset.animations = config.animations
      ? "enabled"
      : "disabled";
    document.documentElement.style.colorScheme = appearance;
  }, [appearance, config.animations]);

  const newTab = useCallback(() => {
    if (windowActionPendingRef.current) {
      return;
    }
    setShowSplash(false);
    dispatch({ type: "new" });
  }, []);

  const completeTabClose = useCallback((tabId: string) => {
    closeTimersRef.current.delete(tabId);
    setClosingTabIds((current) => current.filter((id) => id !== tabId));
    setShowSplash(false);
    setShellExperienceByTab((current) => {
      const next = { ...current };
      delete next[tabId];
      return next;
    });
    dispatch({ type: "close", tabId });
  }, []);

  const closeTab = useCallback(
    (tabId: string) => {
      if (windowActionPendingRef.current) {
        return;
      }
      if (closeTimersRef.current.has(tabId)) {
        return;
      }
      if (!config.animations || systemPrefersReducedMotion()) {
        completeTabClose(tabId);
        return;
      }

      setClosingTabIds((current) => [...current, tabId]);
      const timer = window.setTimeout(
        () => completeTabClose(tabId),
        100,
      );
      closeTimersRef.current.set(tabId, timer);
    },
    [completeTabClose, config.animations],
  );

  useEffect(
    () => () => {
      for (const timer of closeTimersRef.current.values()) {
        window.clearTimeout(timer);
      }
      closeTimersRef.current.clear();
      for (const timer of detachTimersRef.current.values()) {
        window.clearTimeout(timer);
      }
      detachTimersRef.current.clear();
    },
    [],
  );

  useEffect(() => {
    let disposed = false;
    let stopListening: (() => void) | null = null;
    void listenForIncomingTransfers((transfer) => {
      if (disposed || !validIncomingTransfer(transfer)) {
        return;
      }
      if (
        workspaceRef.current.tabs.length >= MAX_TABS ||
        acceptedTransfersRef.current.has(transfer.requestId)
      ) {
        void notifyTransferResult(transfer.sourceWindowLabel, {
          requestId: transfer.requestId,
          ok: false,
          message:
            workspaceRef.current.tabs.length >= MAX_TABS
              ? `The destination already has ${MAX_TABS} tabs.`
              : "This terminal transfer was already received.",
        }).catch(() => undefined);
        return;
      }

      acceptedTransfersRef.current.add(transfer.requestId);
      const action = {
        type: "receiveTransfer",
        title: transfer.title,
        ...(transfer.toIndex === undefined
          ? {}
          : { toIndex: transfer.toIndex }),
        transfer: {
          transferToken: transfer.transferToken,
          requestId: transfer.requestId,
          sourceWindowLabel: transfer.sourceWindowLabel,
        },
      } as const;
      workspaceRef.current = tabWorkspaceReducer(workspaceRef.current, action);
      dispatch(action);
    })
      .then((stop) => {
        if (disposed) stop();
        else stopListening = stop;
      })
      .catch((error: unknown) => {
        if (!disposed && windowContext.supported) {
          setWindowActionError(
            windowErrorMessage(error, "Unable to listen for moved tabs."),
          );
        }
      });

    return () => {
      disposed = true;
      stopListening?.();
    };
  }, [windowContext.supported]);

  const transferTab = useCallback(
    async (tabId: string, destination: TabTransferDestination) => {
      if (windowActionPendingRef.current) return;
      const current = workspaceRef.current;
      const tab = current.tabs.find((candidate) => candidate.id === tabId);
      if (!tab || tab.terminalState.type !== "running") {
        setWindowActionError("Wait for the terminal to finish starting.");
        return;
      }
      if (closeTimersRef.current.has(tab.id)) {
        setWindowActionError("The terminal tab is already closing.");
        return;
      }
      if (destination.type === "new" && current.tabs.length < 2) {
        setWindowActionError(
          "A window must keep at least one tab when another tab moves out.",
        );
        return;
      }

      windowActionPendingRef.current = true;
      setTransferringTabId(tab.id);
      setWindowActionError(null);
      let transferToken: string | null = null;
      let transferResult: Promise<Awaited<ReturnType<typeof waitForTransferResult>>> | null =
        null;
      const transferAbort = new AbortController();
      try {
        const identity = createTransferIdentity();
        const targetWindowLabel =
          destination.type === "new"
            ? identity.targetWindowLabel
            : destination.targetWindowLabel;

        transferToken = await prepareTerminalTransfer(
          tab.terminalState.session.sessionId,
          targetWindowLabel,
        );
        transferResult = waitForTransferResult(
          identity.requestId,
          transferAbort.signal,
        );
        const transfer: IncomingTabTransfer = {
          transferToken,
          requestId: identity.requestId,
          sourceWindowLabel: windowContext.label,
          title: tab.title,
          ...(destination.type === "existing"
            ? { toIndex: destination.toIndex }
            : {}),
        };

        if (destination.type === "new") {
          await createTransferWindow({
            ...transfer,
            targetWindowLabel,
          });
        } else {
          await sendIncomingTransfer(targetWindowLabel, transfer);
        }

        const result = await transferResult;
        if (!result.ok) {
          throw new Error(
            result.message || "The destination rejected the terminal.",
          );
        }

        const closesSourceWindow =
          destination.type === "existing" &&
          workspaceRef.current.tabs.length === 1;
        completeTabClose(tab.id);
        if (closesSourceWindow) {
          window.setTimeout(() => {
            void closeCurrentWindow();
          }, 0);
        }
      } catch (error) {
        transferAbort.abort();
        await transferResult?.catch(() => undefined);
        if (transferToken) {
          await cancelTerminalTransfer(transferToken).catch(() => undefined);
        }
        setWindowActionError(
          windowErrorMessage(error, "Unable to move the terminal tab."),
        );
      } finally {
        transferAbort.abort();
        windowActionPendingRef.current = false;
        setTransferringTabId(null);
      }
    },
    [completeTabClose, windowContext],
  );

  const rememberTabDrag = useCallback((payload: TabDragPayload) => {
    const now = Date.now();
    for (const [dragId, drag] of localTabDragsRef.current) {
      if (now - drag.startedAt > TAB_DRAG_LIFETIME_MS) {
        localTabDragsRef.current.delete(dragId);
      }
    }
    if (localTabDragsRef.current.size >= MAX_TRACKED_TAB_DRAGS) {
      const oldestDragId = localTabDragsRef.current.keys().next().value;
      if (typeof oldestDragId === "string") {
        localTabDragsRef.current.delete(oldestDragId);
      }
    }
    if (handledDragIdsRef.current.size >= MAX_TRACKED_TAB_DRAGS) {
      const oldestHandledId = handledDragIdsRef.current.keys().next().value;
      if (typeof oldestHandledId === "string") {
        handledDragIdsRef.current.delete(oldestHandledId);
      }
    }
    localTabDragsRef.current.set(payload.dragId, {
      tabId: payload.tabId,
      startedAt: now,
    });
  }, []);

  const requestExternalTransfer = useCallback(
    (payload: TabDragPayload, toIndex: number) => {
      if (
        !windowContext.supported ||
        payload.sourceWindowLabel === windowContext.label
      ) {
        return;
      }
      if (workspaceRef.current.tabs.length >= MAX_TABS) {
        setWindowActionError(
          `This window already has the maximum of ${MAX_TABS} tabs.`,
        );
        return;
      }

      setWindowActionError(null);
      const request: TabTransferRequest = {
        dragId: payload.dragId,
        tabId: payload.tabId,
        sourceWindowLabel: payload.sourceWindowLabel,
        targetWindowLabel: windowContext.label,
        toIndex,
      };
      void requestTabTransfer(payload.sourceWindowLabel, request).catch(
        (error: unknown) => {
          setWindowActionError(
            windowErrorMessage(error, "Unable to combine the terminal tab."),
          );
        },
      );
    },
    [windowContext.label, windowContext.supported],
  );

  const detachDraggedTab = useCallback(
    (payload: TabDragPayload) => {
      if (
        !windowContext.supported ||
        payload.sourceWindowLabel !== windowContext.label ||
        handledDragIdsRef.current.has(payload.dragId) ||
        detachTimersRef.current.has(payload.dragId)
      ) {
        return;
      }

      const knownDrag = localTabDragsRef.current.get(payload.dragId);
      const tab = workspaceRef.current.tabs.find(
        (candidate) => candidate.id === payload.tabId,
      );
      if (
        !knownDrag ||
        knownDrag.tabId !== payload.tabId ||
        Date.now() - knownDrag.startedAt > TAB_DRAG_LIFETIME_MS
      ) {
        localTabDragsRef.current.delete(payload.dragId);
        return;
      }
      if (
        workspaceRef.current.tabs.length < 2 ||
        tab?.terminalState.type !== "running" ||
        closeTimersRef.current.has(payload.tabId)
      ) {
        return;
      }

      // A destination's drop event can reach the source just after dragend.
      // Give that request priority before treating the gesture as a tear-off.
      const timer = window.setTimeout(() => {
        detachTimersRef.current.delete(payload.dragId);
        void isCursorOutsideTwominalWindows()
          .then((outside) => {
            if (!outside) {
              return;
            }
            const pendingDrag = localTabDragsRef.current.get(payload.dragId);
            if (
              !pendingDrag ||
              pendingDrag.tabId !== payload.tabId ||
              handledDragIdsRef.current.has(payload.dragId) ||
              windowActionPendingRef.current
            ) {
              localTabDragsRef.current.delete(payload.dragId);
              return;
            }
            localTabDragsRef.current.delete(payload.dragId);
            handledDragIdsRef.current.add(payload.dragId);
            void transferTab(payload.tabId, { type: "new" });
          })
          .catch((error: unknown) => {
            localTabDragsRef.current.delete(payload.dragId);
            setWindowActionError(
              windowErrorMessage(
                error,
                "Unable to determine where the tab was dropped.",
              ),
            );
          });
      }, TAB_DETACH_GRACE_MS);
      detachTimersRef.current.set(payload.dragId, timer);
    },
    [transferTab, windowContext.label, windowContext.supported],
  );

  useEffect(() => {
    let disposed = false;
    let stopListening: (() => void) | null = null;
    void listenForTabTransferRequests((request) => {
      if (
        disposed ||
        !validTabTransferRequest(request) ||
        request.sourceWindowLabel !== windowContext.label ||
        request.targetWindowLabel === windowContext.label ||
        handledDragIdsRef.current.has(request.dragId)
      ) {
        return;
      }

      const knownDrag = localTabDragsRef.current.get(request.dragId);
      if (
        !knownDrag ||
        knownDrag.tabId !== request.tabId ||
        Date.now() - knownDrag.startedAt > TAB_DRAG_LIFETIME_MS
      ) {
        return;
      }

      const detachTimer = detachTimersRef.current.get(request.dragId);
      if (detachTimer !== undefined) {
        window.clearTimeout(detachTimer);
        detachTimersRef.current.delete(request.dragId);
      }
      localTabDragsRef.current.delete(request.dragId);
      handledDragIdsRef.current.add(request.dragId);
      void transferTab(request.tabId, {
        type: "existing",
        targetWindowLabel: request.targetWindowLabel,
        toIndex: request.toIndex,
      });
    })
      .then((stop) => {
        if (disposed) stop();
        else stopListening = stop;
      })
      .catch((error: unknown) => {
        if (!disposed && windowContext.supported) {
          setWindowActionError(
            windowErrorMessage(error, "Unable to listen for dropped tabs."),
          );
        }
      });

    return () => {
      disposed = true;
      stopListening?.();
    };
  }, [transferTab, windowContext.label, windowContext.supported]);

  const restartTab = useCallback((tabId: string) => {
    setShellExperienceByTab((current) => {
      const next = { ...current };
      delete next[tabId];
      return next;
    });
    dispatch({ type: "restart", tabId });
  }, []);

  const updateShellExperience = useCallback(
    (tabId: string, status: ShellExperienceStatus) => {
      setShellExperienceByTab((current) => {
        if (current[tabId] === status) {
          return current;
        }
        return { ...current, [tabId]: status };
      });
    },
    [],
  );

  const updateTerminalState = useCallback(
    (tabId: string, terminalState: TerminalViewState) => {
      const transfer = workspaceRef.current.tabs.find(
        (tab) => tab.id === tabId,
      )?.transfer;
      dispatch({ type: "setTerminalState", tabId, terminalState });
      if (terminalState.type !== "starting") {
        setShowSplash(false);
      }
      if (!transfer || terminalState.type === "starting") {
        return;
      }

      if (terminalState.type === "running") {
        dispatch({ type: "completeTransfer", tabId });
        void notifyTransferResult(transfer.sourceWindowLabel, {
          requestId: transfer.requestId,
          ok: true,
        }).catch(() => undefined);
        return;
      }

      void notifyTransferResult(transfer.sourceWindowLabel, {
        requestId: transfer.requestId,
        ok: false,
        message:
          terminalState.type === "error"
            ? terminalState.message
            : "The transferred terminal is no longer running.",
      })
        .catch(() => undefined)
        .finally(() => {
          completeTabClose(tabId);
          if (windowContext.bootstrap?.requestId === transfer.requestId) {
            window.setTimeout(() => {
              void closeCurrentWindow();
            }, 0);
          }
        });
    },
    [completeTabClose, windowContext.bootstrap?.requestId],
  );

  useEffect(() => {
    const applePlatform = isApplePlatform(
      `${navigator.platform} ${navigator.userAgent}`,
    );
    const handleKeyDown = (event: KeyboardEvent) => {
      const shortcut = tabShortcutFor(event, applePlatform);
      if (settingsOpen) {
        if (shortcut) {
          event.preventDefault();
          event.stopPropagation();
        }
        return;
      }
      if (!shortcut) {
        return;
      }

      let handled = true;
      switch (shortcut.type) {
        case "new":
          newTab();
          break;
        case "close":
          if (workspace.activeId) {
            closeTab(workspace.activeId);
          } else {
            handled = false;
          }
          break;
        case "activateIndex": {
          const tab = workspace.tabs[shortcut.index];
          if (tab) {
            dispatch({ type: "activate", tabId: tab.id });
          } else {
            handled = false;
          }
          break;
        }
        case "previous":
        case "next": {
          if (workspace.tabs.length === 0) {
            handled = false;
            break;
          }
          const currentIndex = Math.max(
            workspace.tabs.findIndex((tab) => tab.id === workspace.activeId),
            0,
          );
          const direction = shortcut.type === "previous" ? -1 : 1;
          const targetIndex =
            (currentIndex + direction + workspace.tabs.length) %
            workspace.tabs.length;
          const target = workspace.tabs[targetIndex];
          if (target) {
            dispatch({ type: "activate", tabId: target.id });
          }
          break;
        }
      }

      if (handled) {
        event.preventDefault();
        event.stopPropagation();
      }
    };

    window.addEventListener("keydown", handleKeyDown, { capture: true });
    return () =>
      window.removeEventListener("keydown", handleKeyDown, { capture: true });
  }, [closeTab, newTab, settingsOpen, workspace.activeId, workspace.tabs]);

  const activeShellExperience = activeTab
    ? shellExperienceByTab[activeTab.id]
    : undefined;
  const activeCwd = activeShellExperience?.cwd ?? null;
  const activeSessionStatus = sessionStatusText(
    activeTab?.terminalState ?? null,
  );

  return (
    <div className="app-shell">
      <header className="topbar">
        <TabStrip
          tabs={workspace.tabs}
          activeTabId={workspace.activeId}
          closingTabIds={closingTabIds}
          onActivate={(tabId) => dispatch({ type: "activate", tabId })}
          onNew={newTab}
          onClose={closeTab}
          onMove={(tabId, toIndex) =>
            dispatch({ type: "move", tabId, toIndex })
          }
          windowLabel={windowContext.supported ? windowContext.label : null}
          onDragStarted={rememberTabDrag}
          onExternalDrop={requestExternalTransfer}
          onDetach={detachDraggedTab}
        />
        <div className="toolbar">
          <button
            type="button"
            className="icon-button settings-button"
            disabled={!configReady}
            onClick={() => setSettingsOpen(true)}
            aria-label="Open settings"
            title="Settings (Command/Ctrl+,)"
          >
            <span aria-hidden="true">⚙</span>
          </button>
        </div>
      </header>

      <main className="terminal-region">
        {workspace.tabs.length === 0 ? (
          <div className="empty-terminal">
            <p>No terminal tabs</p>
            <button type="button" onClick={newTab}>
              New Terminal
            </button>
          </div>
        ) : (
          <div className="terminal-panels">
            {workspace.tabs.map((tab) => {
              const active = tab.id === workspace.activeId;
              return (
                <section
                  className={`terminal-panel${
                    transferringTabId === tab.id ? " is-transferring" : ""
                  }`}
                  id={`panel-${tab.id}`}
                  key={tab.id}
                  role="tabpanel"
                  aria-labelledby={`tab-${tab.id}`}
                  aria-busy={transferringTabId === tab.id}
                  hidden={!active}
                >
                  <TerminalPane
                    appearance={appearance}
                    active={active && transferringTabId !== tab.id}
                    terminalConfig={config.terminal}
                    vimMode={config.vimMode}
                    restartKey={tab.restartKey}
                    history={shellHistory}
                    onCommandAccepted={recordCommand}
                    onShellExperienceChange={(status) =>
                      updateShellExperience(tab.id, status)
                    }
                    onStateChange={(terminalState) =>
                      updateTerminalState(tab.id, terminalState)
                    }
                    transferToken={tab.transfer?.transferToken}
                    onTitleChange={(title) =>
                      dispatch({ type: "setTitle", tabId: tab.id, title })
                    }
                  />
                </section>
              );
            })}
          </div>
        )}

        {activeTab?.terminalState.type === "error" ? (
          <div className="terminal-error" role="alert">
            <p>{activeTab.terminalState.message}</p>
            <button type="button" onClick={() => restartTab(activeTab.id)}>
              Retry
            </button>
          </div>
        ) : null}
      </main>

      <footer className="status-bar">
        <div className="status-left">
          {windowActionError ? (
            <span className="window-action-error" role="alert">
              {windowActionError}
            </span>
          ) : null}
          {activeShellExperience?.phase === "editing" ? (
            activeShellExperience.inputMode ? (
              <span
                className={`input-mode-indicator is-${activeShellExperience.inputMode}`}
                aria-label={`Vim input mode: ${activeShellExperience.inputMode}`}
              >
                {activeShellExperience.inputMode.toUpperCase()}
              </span>
            ) : null
          ) : null}
          {activeCwd ? (
            <span className="status-cwd" title={activeCwd}>
              {activeCwd}
            </span>
          ) : null}
        </div>
        <div className="status-right">
          {activeTab?.terminalState.type === "exited" ? (
            <button
              type="button"
              className="restart-button"
              onClick={() => restartTab(activeTab.id)}
            >
              Restart
            </button>
          ) : null}
          {saveStatus === "error" ? (
            <button
              type="button"
              className="config-error-button"
              title={configError}
              onClick={() => setSettingsOpen(true)}
            >
              Settings unavailable
            </button>
          ) : null}
          {historyStatus === "error" ? (
            <button
              type="button"
              className="config-error-button"
              title={historyError}
              onClick={() => setSettingsOpen(true)}
            >
              History unavailable
            </button>
          ) : null}
          <span className="status-theme" title="Appearance mode">
            {appearanceModeLabel(config.appearance.mode)}
          </span>
          <span className="status-encoding">UTF-8</span>
          <span
            className="status-session"
            role="status"
            title={activeSessionStatus}
          >
            {activeSessionStatus}
          </span>
        </div>
      </footer>

      <SettingsPanel
        config={config}
        open={settingsOpen}
        saveStatus={saveStatus}
        saveError={configError}
        onChange={setConfig}
        onClose={closeSettings}
        onRetry={retrySave}
        historyCount={shellHistory.length}
        historyStatus={historyStatus}
        historyError={historyError}
        onClearHistory={() => {
          void clearHistory().catch(() => undefined);
        }}
        onRetryHistory={retryHistory}
      />
      <SplashScreen visible={showSplash} />
    </div>
  );
}

function getSystemDarkPreference(): boolean {
  return window.matchMedia?.("(prefers-color-scheme: dark)").matches ?? false;
}

function systemPrefersReducedMotion(): boolean {
  return window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ?? false;
}

function appearanceModeLabel(mode: AppearanceMode): string {
  switch (mode) {
    case "system":
      return "System";
    case "light":
      return "Light";
    case "dark":
      return "Dark";
    case "sunSchedule":
      return "Solar";
  }
}

function safeLabel(value: string): string {
  return Array.from(value)
    .filter((character) => {
      const codePoint = character.codePointAt(0) ?? 0;
      return codePoint >= 32 && codePoint !== 127;
    })
    .join("")
    .slice(0, 80);
}

function sessionStatusText(state: TerminalViewState | null): string {
  if (!state) {
    return "No terminal";
  }

  switch (state.type) {
    case "starting":
      return "Starting terminal";
    case "running": {
      const shellName = safeLabel(state.session.shellName) || "Shell";
      return `${shellName} · Connected`;
    }
    case "exited": {
      const shellName = safeLabel(state.session.shellName) || "Shell";
      const exitStatus = state.signal
        ? `Exited: ${safeLabel(state.signal)}`
        : `Exited with code ${state.exitCode}`;
      return `${shellName} · ${exitStatus}`;
    }
    case "error":
      return "Terminal unavailable";
  }
}

function validIncomingTransfer(value: IncomingTabTransfer): boolean {
  return Boolean(
    value &&
      typeof value.transferToken === "string" &&
      value.transferToken.length <= 64 &&
      typeof value.requestId === "string" &&
      value.requestId.length <= 128 &&
      typeof value.sourceWindowLabel === "string" &&
      value.sourceWindowLabel.length <= 128 &&
      typeof value.title === "string" &&
      value.title.length <= 500 &&
      (value.toIndex === undefined ||
        (Number.isFinite(value.toIndex) && value.toIndex >= 0)),
  );
}

function validTabTransferRequest(value: TabTransferRequest): boolean {
  return Boolean(
    value &&
      typeof value.dragId === "string" &&
      value.dragId.length > 0 &&
      value.dragId.length <= 128 &&
      typeof value.tabId === "string" &&
      value.tabId.length > 0 &&
      value.tabId.length <= 128 &&
      typeof value.sourceWindowLabel === "string" &&
      value.sourceWindowLabel.length > 0 &&
      value.sourceWindowLabel.length <= 128 &&
      typeof value.targetWindowLabel === "string" &&
      value.targetWindowLabel.length > 0 &&
      value.targetWindowLabel.length <= 128 &&
      Number.isFinite(value.toIndex) &&
      value.toIndex >= 0,
  );
}

function windowErrorMessage(error: unknown, fallback: string): string {
  if (error instanceof Error && error.message.trim()) {
    return error.message.slice(0, 500);
  }
  if (typeof error === "string" && error.trim()) {
    return error.slice(0, 500);
  }
  return fallback;
}

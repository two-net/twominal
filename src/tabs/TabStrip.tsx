import { useEffect, useRef, useState } from "react";
import type {
  DragEvent as ReactDragEvent,
  KeyboardEvent as ReactKeyboardEvent,
} from "react";
import { MAX_TABS, type TerminalTab } from "./tabState";

export const TAB_DRAG_TYPE = "application/x-twominal-tab";

const TAB_DRAG_VERSION = 1;
const MAX_DRAG_ID_LENGTH = 128;
const MAX_WINDOW_LABEL_LENGTH = 128;

export interface TabDragPayload {
  version: typeof TAB_DRAG_VERSION;
  dragId: string;
  sourceWindowLabel: string;
  tabId: string;
}

export interface TabStripProps {
  tabs: readonly TerminalTab[];
  activeTabId: string | null;
  closingTabIds?: readonly string[];
  onActivate: (tabId: string) => void;
  onNew: () => void;
  onClose: (tabId: string) => void;
  onMove: (tabId: string, toIndex: number) => void;
  windowLabel?: string | null;
  onDragStarted?: (payload: TabDragPayload) => void;
  onExternalDrop?: (payload: TabDragPayload, toIndex: number) => void;
  onDetach?: (payload: TabDragPayload) => void;
}

export function TabStrip({
  tabs,
  activeTabId,
  closingTabIds = [],
  onActivate,
  onNew,
  onClose,
  onMove,
  windowLabel = null,
  onDragStarted,
  onExternalDrop,
  onDetach,
}: TabStripProps) {
  const draggedTab = useRef<{
    tabId: string;
    payload: TabDragPayload | null;
  } | null>(null);
  const localDropHandled = useRef(false);
  const dragCancelled = useRef(false);
  const [dropTargetId, setDropTargetId] = useState<string | null>(null);
  const [dropAtEnd, setDropAtEnd] = useState(false);
  const hasActiveTab = tabs.some((tab) => tab.id === activeTabId);
  const tabLimitReached = tabs.length >= MAX_TABS;

  useEffect(() => {
    const cancelDrag = (event: KeyboardEvent) => {
      if (event.key === "Escape" && draggedTab.current) {
        dragCancelled.current = true;
      }
    };
    window.addEventListener("keydown", cancelDrag, { capture: true });
    return () =>
      window.removeEventListener("keydown", cancelDrag, { capture: true });
  }, []);

  const handleTabKeyDown = (
    event: ReactKeyboardEvent<HTMLButtonElement>,
    tab: TerminalTab,
    index: number,
  ) => {
    if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") {
      return;
    }

    const direction = event.key === "ArrowLeft" ? -1 : 1;
    const isReorderShortcut =
      event.altKey && event.shiftKey && !event.ctrlKey && !event.metaKey;

    if (isReorderShortcut) {
      event.preventDefault();
      const toIndex = index + direction;
      if (toIndex >= 0 && toIndex < tabs.length) {
        onMove(tab.id, toIndex);
        focusTab(tab.id);
      }
      return;
    }

    if (event.altKey || event.ctrlKey || event.metaKey || tabs.length < 2) {
      return;
    }

    event.preventDefault();
    const nextIndex = (index + direction + tabs.length) % tabs.length;
    const nextTab = tabs[nextIndex];
    if (nextTab) {
      onActivate(nextTab.id);
      focusTab(nextTab.id);
    }
  };

  const handleDragStart = (
    event: ReactDragEvent<HTMLButtonElement>,
    tabId: string,
  ) => {
    const payload = windowLabel
      ? createTabDragPayload(tabId, windowLabel)
      : null;
    draggedTab.current = { tabId, payload };
    localDropHandled.current = false;
    dragCancelled.current = false;
    event.dataTransfer.effectAllowed = "move";
    event.dataTransfer.setData(
      TAB_DRAG_TYPE,
      payload ? serializeTabDragPayload(payload) : tabId,
    );
    if (payload) {
      onDragStarted?.(payload);
    }
  };

  const handleDragOver = (
    event: ReactDragEvent<HTMLDivElement>,
    targetTabId: string,
  ) => {
    if (!hasTabDragData(event.dataTransfer, draggedTab.current)) {
      return;
    }
    event.preventDefault();
    event.stopPropagation();
    event.dataTransfer.dropEffect = "move";
    setDropTargetId(
      draggedTab.current?.tabId === targetTabId ? null : targetTabId,
    );
    setDropAtEnd(false);
  };

  const handleDrop = (
    event: ReactDragEvent<HTMLDivElement>,
    toIndex: number,
  ) => {
    event.preventDefault();
    event.stopPropagation();
    handleAcceptedDrop(event.dataTransfer, toIndex);
  };

  const handleAcceptedDrop = (dataTransfer: DataTransfer, toIndex: number) => {
    const localTabId = draggedTab.current?.tabId ?? null;
    const payload = readTabDragPayload(dataTransfer);
    const sameWindowPayload =
      payload && windowLabel && payload.sourceWindowLabel === windowLabel
        ? payload
        : null;
    const tabId = localTabId ?? sameWindowPayload?.tabId ?? null;

    setDropTargetId(null);
    setDropAtEnd(false);

    if (tabId && tabs.some((tab) => tab.id === tabId)) {
      localDropHandled.current = true;
      if (tabs[toIndex]?.id !== tabId) {
        onMove(tabId, toIndex);
        focusTab(tabId);
      }
      return;
    }

    if (
      payload &&
      windowLabel &&
      payload.sourceWindowLabel !== windowLabel
    ) {
      onExternalDrop?.(payload, toIndex);
    }
  };

  const handleListDragOver = (event: ReactDragEvent<HTMLDivElement>) => {
    if (
      event.target !== event.currentTarget ||
      !hasTabDragData(event.dataTransfer, draggedTab.current)
    ) {
      return;
    }
    event.preventDefault();
    event.dataTransfer.dropEffect = "move";
    setDropTargetId(null);
    setDropAtEnd(true);
  };

  const handleListDrop = (event: ReactDragEvent<HTMLDivElement>) => {
    if (event.target !== event.currentTarget) return;
    event.preventDefault();
    handleAcceptedDrop(event.dataTransfer, tabs.length);
  };

  const clearDropState = () => {
    setDropTargetId(null);
    setDropAtEnd(false);
  };

  const handleDragEnd = (event: ReactDragEvent<HTMLButtonElement>) => {
    const completedDrag = draggedTab.current;
    const shouldDetach =
      completedDrag?.payload &&
      !localDropHandled.current &&
      !dragCancelled.current &&
      event.dataTransfer.dropEffect === "none";

    draggedTab.current = null;
    localDropHandled.current = false;
    dragCancelled.current = false;
    clearDropState();

    if (shouldDetach && completedDrag?.payload) {
      onDetach?.(completedDrag.payload);
    }
  };

  return (
    <div className="tab-strip">
      <div
        className={`tab-list${dropAtEnd ? " is-drop-target" : ""}`}
        role="tablist"
        aria-label="Terminal tabs"
        aria-orientation="horizontal"
        onDragOver={handleListDragOver}
        onDragLeave={(event) => {
          const nextTarget = event.relatedTarget;
          if (
            !(nextTarget instanceof Node) ||
            !event.currentTarget.contains(nextTarget)
          ) {
            clearDropState();
          }
        }}
        onDrop={handleListDrop}
      >
        {tabs.map((tab, index) => {
          const isActive = tab.id === activeTabId;
          const label = tab.title.trim() || `Terminal ${index + 1}`;
          const isDropTarget = tab.id === dropTargetId;
          const isClosing = closingTabIds.includes(tab.id);

          return (
            <div
              className={`tab-item${isActive ? " is-active" : ""}${
                isDropTarget ? " is-drop-target" : ""
              }${isClosing ? " is-closing" : ""}`}
              role="presentation"
              key={tab.id}
              onDragOver={(event) => handleDragOver(event, tab.id)}
              onDrop={(event) => handleDrop(event, index)}
            >
              <button
                className="tab-button"
                type="button"
                id={`tab-${tab.id}`}
                role="tab"
                aria-selected={isActive}
                aria-disabled={isClosing}
                aria-controls={`panel-${tab.id}`}
                tabIndex={isActive || (!hasActiveTab && index === 0) ? 0 : -1}
                title={label}
                draggable={!isClosing}
                onClick={() => {
                  if (!isClosing) {
                    onActivate(tab.id);
                  }
                }}
                onKeyDown={(event) => {
                  if (!isClosing) {
                    handleTabKeyDown(event, tab, index);
                  }
                }}
                onDragStart={(event) => handleDragStart(event, tab.id)}
                onDragEnd={handleDragEnd}
              >
                <span className="tab-icon" aria-hidden="true">
                  ›_
                </span>
                <span className="tab-title">{label}</span>
              </button>
              <button
                className="tab-close-button"
                type="button"
                aria-label={`Close ${label}`}
                title={`Close ${label}`}
                disabled={isClosing}
                onClick={() => onClose(tab.id)}
              >
                <span aria-hidden="true">×</span>
              </button>
            </div>
          );
        })}
      </div>

      <button
        className="new-tab-button"
        type="button"
        aria-label="New terminal tab"
        title={
          tabLimitReached
            ? `Maximum of ${MAX_TABS} terminal tabs reached`
            : "New terminal tab"
        }
        disabled={tabLimitReached}
        onClick={onNew}
      >
        <span aria-hidden="true">＋</span>
      </button>
    </div>
  );
}

function focusTab(tabId: string): void {
  document.getElementById(`tab-${tabId}`)?.focus();
}

export function serializeTabDragPayload(payload: TabDragPayload): string {
  return JSON.stringify(payload);
}

export function readTabDragPayload(
  dataTransfer: Pick<DataTransfer, "getData">,
): TabDragPayload | null {
  const value = dataTransfer.getData(TAB_DRAG_TYPE);
  if (!value) return null;

  try {
    const candidate: unknown = JSON.parse(value);
    if (!isTabDragPayload(candidate)) return null;
    return candidate;
  } catch {
    return null;
  }
}

function createTabDragPayload(
  tabId: string,
  sourceWindowLabel: string,
): TabDragPayload {
  return {
    version: TAB_DRAG_VERSION,
    dragId: randomId(),
    sourceWindowLabel,
    tabId,
  };
}

function isTabDragPayload(value: unknown): value is TabDragPayload {
  if (!value || typeof value !== "object") return false;
  const candidate = value as Partial<TabDragPayload>;
  return (
    candidate.version === TAB_DRAG_VERSION &&
    typeof candidate.dragId === "string" &&
    candidate.dragId.length > 0 &&
    candidate.dragId.length <= MAX_DRAG_ID_LENGTH &&
    typeof candidate.sourceWindowLabel === "string" &&
    candidate.sourceWindowLabel.length > 0 &&
    candidate.sourceWindowLabel.length <= MAX_WINDOW_LABEL_LENGTH &&
    typeof candidate.tabId === "string" &&
    candidate.tabId.length > 0 &&
    candidate.tabId.length <= 128
  );
}

function hasTabDragData(
  dataTransfer: Pick<DataTransfer, "types">,
  localDrag: { tabId: string; payload: TabDragPayload | null } | null,
): boolean {
  return Boolean(
    localDrag || Array.from(dataTransfer.types).includes(TAB_DRAG_TYPE),
  );
}

function randomId(): string {
  if (typeof crypto.randomUUID === "function") {
    return crypto.randomUUID();
  }
  return `${Date.now()}-${Math.random().toString(16).slice(2)}`;
}

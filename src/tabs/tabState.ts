import type { TerminalViewState } from "../terminal/types";

export const MAX_TABS = 20;

const MAX_TITLE_CODE_POINTS = 80;

export interface TerminalTab {
  id: string;
  title: string;
  terminalState: TerminalViewState;
  restartKey: number;
  transfer?: TerminalTabTransfer;
}

export interface TerminalTabTransfer {
  transferToken: string;
  requestId: string;
  sourceWindowLabel: string;
}

export interface TabWorkspaceState {
  tabs: TerminalTab[];
  activeId: string | null;
  nextTabNumber: number;
}

export type TabWorkspaceAction =
  | { type: "new" }
  | { type: "close"; tabId: string }
  | { type: "activate"; tabId: string }
  | { type: "move"; tabId: string; toIndex: number }
  | {
      type: "receiveTransfer";
      title: string;
      transfer: TerminalTabTransfer;
      toIndex?: number;
    }
  | { type: "completeTransfer"; tabId: string }
  | {
      type: "setTerminalState";
      tabId: string;
      terminalState: TerminalViewState;
    }
  | { type: "setTitle"; tabId: string; title: string }
  | { type: "restart"; tabId: string };

export function createInitialTabWorkspace(
  transfer?: (TerminalTabTransfer & { title: string }) | null,
): TabWorkspaceState {
  if (transfer) {
    const tabTransfer: TerminalTabTransfer = {
      transferToken: transfer.transferToken,
      requestId: transfer.requestId,
      sourceWindowLabel: transfer.sourceWindowLabel,
    };
    const firstTab = createTransferredTab(1, transfer.title, tabTransfer);
    return {
      tabs: [firstTab],
      activeId: firstTab.id,
      nextTabNumber: 2,
    };
  }
  const firstTab = createTab(1);

  return {
    tabs: [firstTab],
    activeId: firstTab.id,
    nextTabNumber: 2,
  };
}

export function tabWorkspaceReducer(
  state: TabWorkspaceState,
  action: TabWorkspaceAction,
): TabWorkspaceState {
  switch (action.type) {
    case "new":
      return addTab(state);
    case "close":
      return closeTab(state, action.tabId);
    case "activate":
      return activateTab(state, action.tabId);
    case "move":
      return moveTab(state, action.tabId, action.toIndex);
    case "receiveTransfer":
      return receiveTransfer(
        state,
        action.title,
        action.transfer,
        action.toIndex,
      );
    case "completeTransfer":
      return updateTab(state, action.tabId, (tab) => {
        if (!tab.transfer) return tab;
        const completed = { ...tab };
        delete completed.transfer;
        return completed;
      });
    case "setTerminalState":
      return updateTab(state, action.tabId, (tab) => {
        const shellTitle =
          action.terminalState.type === "running"
            ? sanitizeTabTitle(action.terminalState.session.shellName)
            : "";
        const title =
          shellTitle && tab.title === defaultTabTitle(tab.id)
            ? shellTitle
            : tab.title;

        return {
          ...tab,
          terminalState: action.terminalState,
          title,
        };
      });
    case "setTitle": {
      const title = sanitizeTabTitle(action.title);
      return title
        ? updateTab(state, action.tabId, (tab) => ({ ...tab, title }))
        : state;
    }
    case "restart":
      return updateTab(state, action.tabId, (tab) => {
        const restartable = { ...tab };
        delete restartable.transfer;
        return {
          ...restartable,
          title: defaultTabTitle(tab.id),
          terminalState: { type: "starting" },
          restartKey: tab.restartKey + 1,
        };
      });
  }
}

export function sanitizeTabTitle(value: string): string {
  return Array.from(value.trim())
    .filter((character) => !isUnsafeTitleCharacter(character))
    .slice(0, MAX_TITLE_CODE_POINTS)
    .join("")
    .trim();
}

function createTab(tabNumber: number): TerminalTab {
  return {
    id: `tab-${tabNumber}`,
    title: defaultTabTitle(`tab-${tabNumber}`),
    terminalState: { type: "starting" },
    restartKey: 0,
  };
}

function createTransferredTab(
  tabNumber: number,
  title: string,
  transfer: TerminalTabTransfer,
): TerminalTab {
  return {
    ...createTab(tabNumber),
    title: sanitizeTabTitle(title) || `Terminal ${tabNumber}`,
    transfer,
  };
}

function receiveTransfer(
  state: TabWorkspaceState,
  title: string,
  transfer: TerminalTabTransfer,
  toIndex?: number,
): TabWorkspaceState {
  if (
    state.tabs.length >= MAX_TABS ||
    state.tabs.some((tab) => tab.transfer?.requestId === transfer.requestId)
  ) {
    return state;
  }
  const tab = createTransferredTab(state.nextTabNumber, title, transfer);
  const destination =
    toIndex === undefined || !Number.isFinite(toIndex)
      ? state.tabs.length
      : Math.min(Math.max(Math.trunc(toIndex), 0), state.tabs.length);
  const tabs = [...state.tabs];
  tabs.splice(destination, 0, tab);
  return {
    tabs,
    activeId: tab.id,
    nextTabNumber: state.nextTabNumber + 1,
  };
}

function defaultTabTitle(tabId: string): string {
  const tabNumber = tabId.match(/^tab-(\d+)$/)?.[1];
  return tabNumber ? `Terminal ${tabNumber}` : "Terminal";
}

function addTab(state: TabWorkspaceState): TabWorkspaceState {
  if (state.tabs.length >= MAX_TABS) {
    return state;
  }

  const tab = createTab(state.nextTabNumber);
  return {
    tabs: [...state.tabs, tab],
    activeId: tab.id,
    nextTabNumber: state.nextTabNumber + 1,
  };
}

function closeTab(state: TabWorkspaceState, tabId: string): TabWorkspaceState {
  const index = state.tabs.findIndex((tab) => tab.id === tabId);
  if (index === -1) {
    return state;
  }

  const tabs = state.tabs.filter((tab) => tab.id !== tabId);
  if (state.activeId !== tabId) {
    return { ...state, tabs };
  }

  return {
    ...state,
    tabs,
    activeId: tabs[index]?.id ?? tabs[index - 1]?.id ?? null,
  };
}

function activateTab(
  state: TabWorkspaceState,
  tabId: string,
): TabWorkspaceState {
  if (
    tabId === state.activeId ||
    !state.tabs.some((tab) => tab.id === tabId)
  ) {
    return state;
  }

  return { ...state, activeId: tabId };
}

function moveTab(
  state: TabWorkspaceState,
  tabId: string,
  toIndex: number,
): TabWorkspaceState {
  const fromIndex = state.tabs.findIndex((tab) => tab.id === tabId);
  if (fromIndex === -1 || state.tabs.length < 2 || !Number.isFinite(toIndex)) {
    return state;
  }

  const destination = Math.min(
    Math.max(Math.trunc(toIndex), 0),
    state.tabs.length - 1,
  );
  if (fromIndex === destination) {
    return state;
  }

  const tabs = [...state.tabs];
  const [tab] = tabs.splice(fromIndex, 1);
  tabs.splice(destination, 0, tab);
  return { ...state, tabs };
}

function updateTab(
  state: TabWorkspaceState,
  tabId: string,
  update: (tab: TerminalTab) => TerminalTab,
): TabWorkspaceState {
  const index = state.tabs.findIndex((tab) => tab.id === tabId);
  if (index === -1) {
    return state;
  }

  const tabs = [...state.tabs];
  tabs[index] = update(tabs[index]);
  return { ...state, tabs };
}

function isUnsafeTitleCharacter(character: string): boolean {
  const codePoint = character.codePointAt(0) ?? 0;

  return (
    codePoint <= 0x1f ||
    (codePoint >= 0x7f && codePoint <= 0x9f) ||
    codePoint === 0x061c ||
    codePoint === 0x200e ||
    codePoint === 0x200f ||
    (codePoint >= 0x202a && codePoint <= 0x202e) ||
    (codePoint >= 0x2066 && codePoint <= 0x206f) ||
    (codePoint >= 0xd800 && codePoint <= 0xdfff) ||
    codePoint === 0xfeff
  );
}

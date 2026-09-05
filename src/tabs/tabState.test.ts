import type { TerminalViewState } from "../terminal/types";
import { describe, expect, it } from "vitest";
import {
  createInitialTabWorkspace,
  MAX_TABS,
  sanitizeTabTitle,
  tabWorkspaceReducer,
  type TabWorkspaceState,
} from "./tabState";

describe("tabWorkspaceReducer", () => {
  it("creates one active starting tab", () => {
    expect(createInitialTabWorkspace()).toEqual({
      tabs: [
        {
          id: "tab-1",
          title: "Terminal 1",
          terminalState: { type: "starting" },
          restartKey: 0,
        },
      ],
      activeId: "tab-1",
      nextTabNumber: 2,
    });
  });

  it("adds active tabs with monotonic identities and enforces the cap", () => {
    let state = createInitialTabWorkspace();
    state = tabWorkspaceReducer(state, { type: "new" });
    state = tabWorkspaceReducer(state, { type: "close", tabId: "tab-2" });
    state = tabWorkspaceReducer(state, { type: "new" });

    expect(state.tabs.map((tab) => tab.id)).toEqual(["tab-1", "tab-3"]);
    expect(state.activeId).toBe("tab-3");
    expect(state.nextTabNumber).toBe(4);

    while (state.tabs.length < MAX_TABS) {
      state = tabWorkspaceReducer(state, { type: "new" });
    }
    const fullState = state;

    expect(tabWorkspaceReducer(state, { type: "new" })).toBe(fullState);
    expect(state.tabs).toHaveLength(MAX_TABS);
  });

  it("closes the active tab by selecting its right neighbor, then its left", () => {
    let state = stateWithTabs(3);
    state = tabWorkspaceReducer(state, {
      type: "activate",
      tabId: "tab-2",
    });
    state = tabWorkspaceReducer(state, { type: "close", tabId: "tab-2" });

    expect(state.tabs.map((tab) => tab.id)).toEqual(["tab-1", "tab-3"]);
    expect(state.activeId).toBe("tab-3");

    state = tabWorkspaceReducer(state, { type: "close", tabId: "tab-3" });
    expect(state.activeId).toBe("tab-1");

    state = tabWorkspaceReducer(state, { type: "close", tabId: "tab-1" });
    expect(state.tabs).toEqual([]);
    expect(state.activeId).toBeNull();
  });

  it("does not change the active tab when closing an inactive tab", () => {
    const state = stateWithTabs(3);
    const next = tabWorkspaceReducer(state, {
      type: "close",
      tabId: "tab-1",
    });

    expect(next.activeId).toBe("tab-3");
  });

  it("activates existing tabs and ignores unknown tabs", () => {
    const state = stateWithTabs(2);
    const active = tabWorkspaceReducer(state, {
      type: "activate",
      tabId: "tab-1",
    });

    expect(active.activeId).toBe("tab-1");
    expect(
      tabWorkspaceReducer(active, { type: "activate", tabId: "missing" }),
    ).toBe(active);
  });

  it("moves tabs to a clamped integer index without changing the active tab", () => {
    const state = stateWithTabs(3);
    const moved = tabWorkspaceReducer(state, {
      type: "move",
      tabId: "tab-1",
      toIndex: 99.8,
    });

    expect(moved.tabs.map((tab) => tab.id)).toEqual([
      "tab-2",
      "tab-3",
      "tab-1",
    ]);
    expect(moved.activeId).toBe("tab-3");
  });

  it("boots directly into a transferred tab and completes its one-time attachment", () => {
    const state = createInitialTabWorkspace({
      transferToken: "token-1",
      requestId: "request-1",
      sourceWindowLabel: "main",
      title: "project shell",
    });

    expect(state.tabs).toEqual([
      expect.objectContaining({
        id: "tab-1",
        title: "project shell",
        transfer: {
          transferToken: "token-1",
          requestId: "request-1",
          sourceWindowLabel: "main",
        },
      }),
    ]);

    const completed = tabWorkspaceReducer(state, {
      type: "completeTransfer",
      tabId: "tab-1",
    });
    expect(completed.tabs[0].transfer).toBeUndefined();
  });

  it("receives transferred tabs with local collision-free IDs and ignores duplicates", () => {
    const state = createInitialTabWorkspace();
    const transfer = {
      transferToken: "token-2",
      requestId: "request-2",
      sourceWindowLabel: "twominal-child",
    };
    const received = tabWorkspaceReducer(state, {
      type: "receiveTransfer",
      title: "remote shell",
      transfer,
    });

    expect(received.tabs.map((tab) => tab.id)).toEqual(["tab-1", "tab-2"]);
    expect(received.activeId).toBe("tab-2");
    expect(
      tabWorkspaceReducer(received, {
        type: "receiveTransfer",
        title: "duplicate",
        transfer,
      }),
    ).toBe(received);
  });

  it("inserts transferred tabs at a clamped drop index", () => {
    const state = stateWithTabs(2);
    const received = tabWorkspaceReducer(state, {
      type: "receiveTransfer",
      title: "remote shell",
      toIndex: -20,
      transfer: {
        transferToken: "token-3",
        requestId: "request-3",
        sourceWindowLabel: "twominal-child",
      },
    });

    expect(received.tabs.map((tab) => tab.id)).toEqual([
      "tab-3",
      "tab-1",
      "tab-2",
    ]);
    expect(received.activeId).toBe("tab-3");

    const appended = tabWorkspaceReducer(received, {
      type: "receiveTransfer",
      title: "another remote shell",
      toIndex: 999,
      transfer: {
        transferToken: "token-4",
        requestId: "request-4",
        sourceWindowLabel: "twominal-other",
      },
    });
    expect(appended.tabs.at(-1)?.title).toBe("another remote shell");
  });

  it("updates terminal state and derives running titles from safe shell names", () => {
    const state = createInitialTabWorkspace();
    const running: TerminalViewState = {
      type: "running",
      session: {
        sessionId: "session-1",
        shellName: "\u202ezsh\u001b",
        cwd: "/tmp",
        shellIntegration: false,
        shellIntegrationNonce: null,
      },
    };
    const next = tabWorkspaceReducer(state, {
      type: "setTerminalState",
      tabId: "tab-1",
      terminalState: running,
    });

    expect(next.tabs[0].terminalState).toBe(running);
    expect(next.tabs[0].title).toBe("zsh");
  });

  it("keeps the existing title when a running shell name sanitizes to empty", () => {
    const state = createInitialTabWorkspace();
    const next = tabWorkspaceReducer(state, {
      type: "setTerminalState",
      tabId: "tab-1",
      terminalState: {
        type: "running",
        session: {
          sessionId: "session-1",
          shellName: "\u0000\u202e",
          cwd: null,
          shellIntegration: false,
          shellIntegrationNonce: null,
        },
      },
    });

    expect(next.tabs[0].title).toBe("Terminal 1");
  });

  it("does not overwrite a dynamic title received while the session starts", () => {
    let state = createInitialTabWorkspace();
    state = tabWorkspaceReducer(state, {
      type: "setTitle",
      tabId: "tab-1",
      title: "~/projects/twominal",
    });
    state = tabWorkspaceReducer(state, {
      type: "setTerminalState",
      tabId: "tab-1",
      terminalState: {
        type: "running",
        session: {
          sessionId: "session-1",
          shellName: "zsh",
          cwd: null,
          shellIntegration: false,
          shellIntegrationNonce: null,
        },
      },
    });

    expect(state.tabs[0].title).toBe("~/projects/twominal");
  });

  it("sanitizes dynamic titles, counts Unicode code points, and ignores empty ones", () => {
    const state = createInitialTabWorkspace();
    const longTitle = `  hello\u0007\u2066world ${"😀".repeat(100)}  `;
    const next = tabWorkspaceReducer(state, {
      type: "setTitle",
      tabId: "tab-1",
      title: longTitle,
    });

    expect(next.tabs[0].title.startsWith("helloworld ")).toBe(true);
    expect(Array.from(next.tabs[0].title)).toHaveLength(80);
    expect(
      tabWorkspaceReducer(next, {
        type: "setTitle",
        tabId: "tab-1",
        title: " \u0000\u202e ",
      }),
    ).toBe(next);
  });

  it("restarts only the requested terminal", () => {
    let state = stateWithTabs(2);
    state = tabWorkspaceReducer(state, {
      type: "setTitle",
      tabId: "tab-1",
      title: "old process title",
    });
    const next = tabWorkspaceReducer(state, {
      type: "restart",
      tabId: "tab-1",
    });

    expect(next.tabs[0].restartKey).toBe(1);
    expect(next.tabs[0].title).toBe("Terminal 1");
    expect(next.tabs[0].terminalState).toEqual({ type: "starting" });
    expect(next.tabs[1]).toBe(state.tabs[1]);
  });

  it("returns the same state for actions targeting unknown tabs", () => {
    const state = createInitialTabWorkspace();

    expect(
      tabWorkspaceReducer(state, { type: "close", tabId: "missing" }),
    ).toBe(state);
    expect(
      tabWorkspaceReducer(state, {
        type: "move",
        tabId: "missing",
        toIndex: 0,
      }),
    ).toBe(state);
    expect(
      tabWorkspaceReducer(state, { type: "restart", tabId: "missing" }),
    ).toBe(state);
  });
});

describe("sanitizeTabTitle", () => {
  it("removes terminal controls, bidi controls, and byte-order marks", () => {
    expect(
      sanitizeTabTitle(
        "\u001b\u009b\u061c\u200f\u202aSafe\u202e\u2066\u206f\ufeff",
      ),
    ).toBe("Safe");
  });
});

function stateWithTabs(count: number): TabWorkspaceState {
  let state = createInitialTabWorkspace();
  while (state.tabs.length < count) {
    state = tabWorkspaceReducer(state, { type: "new" });
  }
  return state;
}

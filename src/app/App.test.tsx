import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ShellExperienceStatus } from "../shell";
import { isApplePlatform } from "../tabs/shortcuts";
import { MAX_TABS } from "../tabs/tabState";
import {
  readTabDragPayload,
  serializeTabDragPayload,
  TAB_DRAG_TYPE,
  type TabDragPayload,
} from "../tabs/TabStrip";
import type { TerminalViewState } from "../terminal/types";
import { App } from "./App";

const windowMocks = vi.hoisted(() => ({
  context: {
    supported: false,
    label: "browser",
    bootstrap: null,
  },
  incomingListener: null as
    | ((transfer: {
        transferToken: string;
        requestId: string;
        sourceWindowLabel: string;
        title: string;
        toIndex?: number;
      }) => void)
    | null,
  dragRequestListener: null as
    | ((request: {
        dragId: string;
        tabId: string;
        sourceWindowLabel: string;
        targetWindowLabel: string;
        toIndex: number;
      }) => void)
    | null,
  closeCurrentWindow: vi.fn().mockResolvedValue(undefined),
  createTransferWindow: vi.fn().mockResolvedValue(undefined),
  isCursorOutsideTwominalWindows: vi.fn().mockResolvedValue(true),
  notifyTransferResult: vi.fn().mockResolvedValue(undefined),
  requestTabTransfer: vi.fn().mockResolvedValue(undefined),
  sendIncomingTransfer: vi.fn().mockResolvedValue(undefined),
  waitForTransferResult: vi.fn().mockResolvedValue({
    requestId: "request-1",
    ok: true,
  }),
}));

const terminalTransferMocks = vi.hoisted(() => ({
  prepare: vi.fn().mockResolvedValue("transfer-token"),
  cancel: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("../windows/windowRuntime", () => ({
  getWindowContext: () => windowMocks.context,
  createTransferIdentity: () => ({
    requestId: "request-1",
    targetWindowLabel: "twominal-request-1",
  }),
  closeCurrentWindow: windowMocks.closeCurrentWindow,
  createTransferWindow: windowMocks.createTransferWindow,
  isCursorOutsideTwominalWindows:
    windowMocks.isCursorOutsideTwominalWindows,
  listenForIncomingTransfers: vi.fn(
    async (listener: typeof windowMocks.incomingListener) => {
      windowMocks.incomingListener = listener;
      return () => undefined;
    },
  ),
  listenForTabTransferRequests: vi.fn(
    async (listener: typeof windowMocks.dragRequestListener) => {
      windowMocks.dragRequestListener = listener;
      return () => undefined;
    },
  ),
  notifyTransferResult: windowMocks.notifyTransferResult,
  requestTabTransfer: windowMocks.requestTabTransfer,
  sendIncomingTransfer: windowMocks.sendIncomingTransfer,
  waitForTransferResult: windowMocks.waitForTransferResult,
}));

vi.mock("../terminal/terminalClient", () => ({
  prepareTerminalTransfer: terminalTransferMocks.prepare,
  cancelTerminalTransfer: terminalTransferMocks.cancel,
}));

vi.mock("./useAppConfig", () => ({
  useAppConfig: () => ({
    config: {
      schemaVersion: 1,
      appearance: { mode: "system", latitude: null, longitude: null },
      terminal: {
        fontFamily: "monospace",
        fontSize: 14,
        lineHeight: 1.18,
        letterSpacing: 0,
        fontWeight: 400,
        fontLigatures: true,
      },
      animations: true,
      vimMode: false,
    },
    setConfig: vi.fn(),
    ready: true,
    saveStatus: "saved",
    retrySave: vi.fn(),
  }),
}));

vi.mock("./useShellHistory", () => ({
  useShellHistory: () => ({
    entries: [],
    status: "ready",
    recordCommand: vi.fn(),
    clearHistory: vi.fn(() => Promise.resolve()),
    retryLoad: vi.fn(),
  }),
}));

vi.mock("../terminal/TerminalPane", () => ({
  TerminalPane: ({
    active,
    onShellExperienceChange,
    onStateChange,
    transferToken,
  }: {
    active: boolean;
    onShellExperienceChange: (status: ShellExperienceStatus) => void;
    onStateChange: (state: TerminalViewState) => void;
    transferToken?: string;
  }) => (
    <div
      data-testid="terminal-pane"
      data-active={String(active)}
      data-transfer-token={transferToken}
    >
      <button
        type="button"
        aria-label="Report Vim mode"
        onClick={() => {
          onShellExperienceChange({
            phase: "editing",
            suggestionAvailable: false,
            completionCount: 0,
            tokenKind: "command",
            inputMode: "normal",
            cwd: "/Users/two/Projects/twominal",
          });
          onStateChange({
            type: "running",
            session: {
              sessionId: "session-zsh",
              shellName: "zsh",
              cwd: "/Users/two/Projects/twominal",
              shellIntegration: true,
              shellIntegrationNonce: "nonce-zsh",
            },
          });
        }}
      />
      <button
        type="button"
        aria-label="Report home shell"
        onClick={() => {
          onShellExperienceChange({
            phase: "waiting",
            suggestionAvailable: false,
            completionCount: 0,
            tokenKind: null,
            inputMode: null,
            cwd: "/Users/two",
          });
          onStateChange({
            type: "running",
            session: {
              sessionId: "session-fish",
              shellName: "fish",
              cwd: "/Users/two",
              shellIntegration: true,
              shellIntegrationNonce: "nonce-fish",
            },
          });
        }}
      />
    </div>
  ),
}));

afterEach(() => {
  cleanup();
  windowMocks.context = {
    supported: false,
    label: "browser",
    bootstrap: null,
  };
  windowMocks.incomingListener = null;
  windowMocks.dragRequestListener = null;
  vi.clearAllMocks();
});

describe("App tab workspace", () => {
  it("places tabs and settings together in the unified top bar", () => {
    render(<App />);

    const tabList = screen.getByRole("tablist", { name: "Terminal tabs" });
    const topbar = tabList.closest("header");
    const settingsButton = screen.getByRole("button", {
      name: "Open settings",
    });

    expect(topbar).toHaveClass("topbar");
    expect(topbar).toContainElement(settingsButton);
    expect(settingsButton).toHaveClass("icon-button", "settings-button");
    expect(settingsButton.querySelector("[aria-hidden='true']")).toHaveTextContent(
      "⚙",
    );
    expect(
      screen.queryByRole("button", { name: "Move active tab to new window" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", {
        name: "Move active tab into another window",
      }),
    ).not.toBeInTheDocument();
  });

  it("keeps every terminal mounted while switching and closing tabs", async () => {
    render(<App />);

    expect(screen.getAllByRole("tab")).toHaveLength(1);
    fireEvent.click(screen.getByRole("button", { name: "New terminal tab" }));

    const tabs = screen.getAllByRole("tab");
    const panes = screen.getAllByTestId("terminal-pane");
    expect(tabs).toHaveLength(2);
    expect(tabs[1]).toHaveAttribute("aria-selected", "true");
    expect(panes).toHaveLength(2);
    expect(panes[0].closest("[role='tabpanel']")).toHaveAttribute("hidden");
    expect(panes[1].closest("[role='tabpanel']")).not.toHaveAttribute(
      "hidden",
    );

    fireEvent.click(
      screen.getByRole("button", { name: "Close Terminal 2" }),
    );
    await waitFor(() => expect(screen.getAllByRole("tab")).toHaveLength(1));
    expect(screen.getAllByTestId("terminal-pane")).toHaveLength(1);
  });

  it("supports platform tab shortcuts and a recoverable zero-tab state", async () => {
    render(<App />);
    const primaryModifier = isApplePlatform(
      `${navigator.platform} ${navigator.userAgent}`,
    )
      ? { metaKey: true }
      : { ctrlKey: true };

    fireEvent.keyDown(window, {
      code: "KeyT",
      ...primaryModifier,
    });
    expect(screen.getAllByRole("tab")).toHaveLength(2);

    fireEvent.keyDown(window, {
      code: "Digit1",
      ...primaryModifier,
    });
    expect(screen.getByRole("tab", { name: "Terminal 1" })).toHaveAttribute(
      "aria-selected",
      "true",
    );

    fireEvent.keyDown(window, {
      code: "KeyW",
      ...primaryModifier,
    });
    await waitFor(() =>
      expect(screen.getByRole("tab", { name: "Terminal 2" })).toHaveAttribute(
        "aria-selected",
        "true",
      ),
    );

    fireEvent.keyDown(window, {
      code: "KeyW",
      ...primaryModifier,
    });
    await waitFor(() => expect(screen.queryAllByRole("tab")).toHaveLength(0));
    expect(screen.getByText("No terminal tabs")).toBeInTheDocument();

    fireEvent.keyDown(window, {
      code: "KeyT",
      ...primaryModifier,
    });
    expect(screen.getByRole("tab", { name: "Terminal 3" })).toBeVisible();
  });

  it("opens settings with the platform shortcut without changing tabs", () => {
    render(<App />);
    const primaryModifier = isApplePlatform(
      `${navigator.platform} ${navigator.userAgent}`,
    )
      ? { metaKey: true }
      : { ctrlKey: true };

    fireEvent.keyDown(window, {
      code: "Comma",
      ...primaryModifier,
    });
    expect(
      screen.getByRole("dialog", { name: "Twominal Settings" }),
    ).toBeVisible();

    fireEvent.keyDown(window, {
      code: "KeyT",
      ...primaryModifier,
    });
    expect(screen.getAllByRole("tab")).toHaveLength(1);

    fireEvent.keyDown(document, { key: "Escape" });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("shows the active tab's Vim input mode in the footer", () => {
    render(<App />);

    fireEvent.click(screen.getByRole("button", { name: "Report Vim mode" }));
    expect(screen.getByLabelText("Vim input mode: normal")).toHaveTextContent(
      "NORMAL",
    );
  });

  it("shows only the active tab's live cwd and shell status in the footer", () => {
    render(<App />);

    fireEvent.click(screen.getByRole("button", { name: "Report Vim mode" }));
    expect(screen.getByText("/Users/two/Projects/twominal")).toHaveClass(
      "status-cwd",
    );
    expect(screen.getByText("System")).toHaveClass("status-theme");
    expect(screen.getByText("UTF-8")).toHaveClass("status-encoding");
    expect(screen.getByRole("status")).toHaveTextContent("zsh · Connected");

    fireEvent.click(screen.getByRole("button", { name: "New terminal tab" }));
    expect(
      screen.queryByText("/Users/two/Projects/twominal"),
    ).not.toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent("Starting terminal");

    fireEvent.click(
      screen.getByRole("button", { name: "Report home shell" }),
    );
    expect(screen.getByText("/Users/two")).toHaveClass("status-cwd");
    expect(screen.getByRole("status")).toHaveTextContent(
      "fish · Connected",
    );

    fireEvent.click(screen.getByRole("tab", { name: "zsh" }));
    expect(screen.getByText("/Users/two/Projects/twominal")).toBeVisible();
    expect(screen.getByRole("status")).toHaveTextContent("zsh · Connected");
  });

  it("detaches a running tab dropped outside while keeping one source tab", async () => {
    windowMocks.context = {
      supported: true,
      label: "main",
      bootstrap: null,
    };
    render(<App />);

    fireEvent.click(screen.getByRole("button", { name: "New terminal tab" }));
    fireEvent.click(screen.getByRole("button", { name: "Report Vim mode" }));
    const draggedTab = screen.getByRole("tab", { name: "zsh" });
    dragOutside(draggedTab);

    await waitFor(() => expect(screen.getAllByRole("tab")).toHaveLength(1));
    expect(windowMocks.isCursorOutsideTwominalWindows).toHaveBeenCalledOnce();
    expect(terminalTransferMocks.prepare).toHaveBeenCalledWith(
      "session-zsh",
      "twominal-request-1",
    );
    expect(windowMocks.createTransferWindow).toHaveBeenCalledWith(
      expect.objectContaining({
        transferToken: "transfer-token",
        requestId: "request-1",
        sourceWindowLabel: "main",
        targetWindowLabel: "twominal-request-1",
      }),
    );
    expect(terminalTransferMocks.cancel).not.toHaveBeenCalled();
  });

  it("does not detach the only tab in a window", () => {
    windowMocks.context = {
      supported: true,
      label: "main",
      bootstrap: null,
    };
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "Report Vim mode" }));

    dragOutside(screen.getByRole("tab", { name: "zsh" }));

    expect(screen.getAllByRole("tab")).toHaveLength(1);
    expect(windowMocks.isCursorOutsideTwominalWindows).not.toHaveBeenCalled();
    expect(terminalTransferMocks.prepare).not.toHaveBeenCalled();
  });

  it("keeps a tab put when an unclaimed drop is inside a Twominal window", async () => {
    windowMocks.context = {
      supported: true,
      label: "main",
      bootstrap: null,
    };
    windowMocks.isCursorOutsideTwominalWindows.mockResolvedValueOnce(false);
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "New terminal tab" }));
    fireEvent.click(screen.getByRole("button", { name: "Report Vim mode" }));

    dragOutside(screen.getByRole("tab", { name: "zsh" }));

    await waitFor(() =>
      expect(windowMocks.isCursorOutsideTwominalWindows).toHaveBeenCalledOnce(),
    );
    expect(screen.getAllByRole("tab")).toHaveLength(2);
    expect(terminalTransferMocks.prepare).not.toHaveBeenCalled();
    expect(windowMocks.createTransferWindow).not.toHaveBeenCalled();
  });

  it("requests a transfer when another window's tab is dropped on this strip", async () => {
    windowMocks.context = {
      supported: true,
      label: "main",
      bootstrap: null,
    };
    render(<App />);
    const payload: TabDragPayload = {
      version: 1,
      dragId: "drag-from-child",
      sourceWindowLabel: "twominal-child",
      tabId: "tab-7",
    };
    const dataTransfer = createDataTransfer({
      [TAB_DRAG_TYPE]: serializeTabDragPayload(payload),
    });
    const tabList = screen.getByRole("tablist", { name: "Terminal tabs" });

    fireEvent.dragOver(tabList, { dataTransfer });
    fireEvent.drop(tabList, { dataTransfer });

    await waitFor(() =>
      expect(windowMocks.requestTabTransfer).toHaveBeenCalledWith(
        "twominal-child",
        {
          dragId: "drag-from-child",
          tabId: "tab-7",
          sourceWindowLabel: "twominal-child",
          targetWindowLabel: "main",
          toIndex: 1,
        },
      ),
    );
    expect(screen.getAllByRole("tab")).toHaveLength(1);
  });

  it("rejects an external drop when the destination is at its tab limit", () => {
    windowMocks.context = {
      supported: true,
      label: "main",
      bootstrap: null,
    };
    render(<App />);
    const newTabButton = screen.getByRole("button", {
      name: "New terminal tab",
    });
    for (let tabCount = 1; tabCount < MAX_TABS; tabCount += 1) {
      fireEvent.click(newTabButton);
    }
    expect(screen.getAllByRole("tab")).toHaveLength(MAX_TABS);

    const payload: TabDragPayload = {
      version: 1,
      dragId: "drag-into-full-window",
      sourceWindowLabel: "twominal-child",
      tabId: "tab-7",
    };
    const dataTransfer = createDataTransfer({
      [TAB_DRAG_TYPE]: serializeTabDragPayload(payload),
    });
    const tabList = screen.getByRole("tablist", { name: "Terminal tabs" });
    fireEvent.dragOver(tabList, { dataTransfer });
    fireEvent.drop(tabList, { dataTransfer });

    expect(screen.getByRole("alert")).toHaveTextContent(
      `maximum of ${MAX_TABS} tabs`,
    );
    expect(windowMocks.requestTabTransfer).not.toHaveBeenCalled();
  });

  it("ignores a cross-window request that has no matching live drag", async () => {
    windowMocks.context = {
      supported: true,
      label: "main",
      bootstrap: null,
    };
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "Report Vim mode" }));
    await waitFor(() => expect(windowMocks.dragRequestListener).not.toBeNull());

    act(() => {
      windowMocks.dragRequestListener?.({
        dragId: "fabricated-drag",
        tabId: "tab-1",
        sourceWindowLabel: "main",
        targetWindowLabel: "twominal-child",
        toIndex: 0,
      });
    });

    expect(terminalTransferMocks.prepare).not.toHaveBeenCalled();
    expect(screen.getAllByRole("tab")).toHaveLength(1);
  });

  it("moves a dragged tab into its drop target and closes the empty source window", async () => {
    windowMocks.context = {
      supported: true,
      label: "twominal-child",
      bootstrap: null,
    };
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "Report Vim mode" }));
    await waitFor(() => expect(windowMocks.dragRequestListener).not.toBeNull());
    const draggedTab = screen.getByRole("tab", { name: "zsh" });
    const dataTransfer = createDataTransfer();
    fireEvent.dragStart(draggedTab, { dataTransfer });
    const payload = readTabDragPayload(dataTransfer);
    expect(payload).not.toBeNull();
    endDragOutside(draggedTab, dataTransfer);

    act(() => {
      const request = {
        dragId: payload?.dragId ?? "missing",
        tabId: payload?.tabId ?? "missing",
        sourceWindowLabel: "twominal-child",
        targetWindowLabel: "main",
        toIndex: 2,
      };
      windowMocks.dragRequestListener?.(request);
      windowMocks.dragRequestListener?.(request);
    });

    await waitFor(() => expect(screen.queryAllByRole("tab")).toHaveLength(0));
    expect(terminalTransferMocks.prepare).toHaveBeenCalledWith(
      "session-zsh",
      "main",
    );
    expect(terminalTransferMocks.prepare).toHaveBeenCalledOnce();
    expect(windowMocks.isCursorOutsideTwominalWindows).not.toHaveBeenCalled();
    expect(windowMocks.sendIncomingTransfer).toHaveBeenCalledWith(
      "main",
      expect.objectContaining({
        transferToken: "transfer-token",
        sourceWindowLabel: "twominal-child",
        toIndex: 2,
      }),
    );
    await waitFor(() => expect(windowMocks.closeCurrentWindow).toHaveBeenCalled());
  });

  it("keeps the source tab and cancels its token when a destination rejects it", async () => {
    windowMocks.context = {
      supported: true,
      label: "main",
      bootstrap: null,
    };
    windowMocks.waitForTransferResult.mockResolvedValueOnce({
      requestId: "request-1",
      ok: false,
      message: "Destination is full.",
    });
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "New terminal tab" }));
    fireEvent.click(screen.getByRole("button", { name: "Report Vim mode" }));

    dragOutside(screen.getByRole("tab", { name: "zsh" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Destination is full.",
    );
    expect(screen.getAllByRole("tab")).toHaveLength(2);
    expect(terminalTransferMocks.cancel).toHaveBeenCalledWith("transfer-token");
  });

  it("accepts an incoming live terminal and confirms attachment to its source", async () => {
    windowMocks.context = {
      supported: true,
      label: "main",
      bootstrap: null,
    };
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "New terminal tab" }));
    await waitFor(() => expect(windowMocks.incomingListener).not.toBeNull());

    windowMocks.incomingListener?.({
      transferToken: "incoming-token",
      requestId: "incoming-request",
      sourceWindowLabel: "twominal-child",
      title: "remote zsh",
      toIndex: 0,
    });

    await waitFor(() => expect(screen.getAllByRole("tab")).toHaveLength(3));
    expect(screen.getAllByRole("tab").map((tab) => tab.textContent)).toEqual([
      expect.stringContaining("remote zsh"),
      expect.stringContaining("Terminal 1"),
      expect.stringContaining("Terminal 2"),
    ]);
    expect(screen.getByRole("tab", { name: "remote zsh" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(screen.getAllByTestId("terminal-pane")[0]).toHaveAttribute(
      "data-transfer-token",
      "incoming-token",
    );

    fireEvent.click(screen.getByRole("button", { name: "Report Vim mode" }));
    await waitFor(() =>
      expect(windowMocks.notifyTransferResult).toHaveBeenCalledWith(
        "twominal-child",
        { requestId: "incoming-request", ok: true },
      ),
    );
  });
});

function dragOutside(tab: HTMLElement) {
  const dataTransfer = createDataTransfer();
  fireEvent.dragStart(tab, { dataTransfer });
  endDragOutside(tab, dataTransfer);
  return dataTransfer;
}

function endDragOutside(
  tab: HTMLElement,
  dataTransfer: ReturnType<typeof createDataTransfer>,
) {
  fireEvent.dragEnd(tab, { dataTransfer });
}

function createDataTransfer(initial: Record<string, string> = {}) {
  const data = new Map(Object.entries(initial));
  return {
    dropEffect: "none",
    effectAllowed: "uninitialized",
    get types() {
      return [...data.keys()];
    },
    getData: vi.fn((type: string) => data.get(type) ?? ""),
    setData: vi.fn((type: string, value: string) => {
      data.set(type, value);
    }),
    clearData: vi.fn((type?: string) => {
      if (type) data.delete(type);
      else data.clear();
    }),
  };
}

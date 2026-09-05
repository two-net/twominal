import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  createTransferWindow,
  isCursorOutsideTwominalWindows,
  requestTabTransfer,
} from "./windowRuntime";

const tauriMocks = vi.hoisted(() => ({
  constructedWindows: [] as Array<{
    label: string;
    options: Record<string, unknown>;
  }>,
  cursorPosition: vi.fn(),
  emitTo: vi.fn().mockResolvedValue(undefined),
  getAllWebviewWindows: vi.fn(),
  currentWindow: {
    label: "main",
    close: vi.fn().mockResolvedValue(undefined),
    listen: vi.fn(),
  },
}));

vi.mock("@tauri-apps/api/event", () => ({
  emitTo: tauriMocks.emitTo,
}));

vi.mock("@tauri-apps/api/window", () => ({
  cursorPosition: tauriMocks.cursorPosition,
}));

vi.mock("@tauri-apps/api/webviewWindow", () => ({
  getAllWebviewWindows: tauriMocks.getAllWebviewWindows,
  getCurrentWebviewWindow: () => tauriMocks.currentWindow,
  WebviewWindow: class MockWebviewWindow {
    constructor(label: string, options: Record<string, unknown>) {
      tauriMocks.constructedWindows.push({ label, options });
    }

    async once(
      eventName: string,
      listener: (event: { payload: unknown }) => void,
    ) {
      if (eventName === "tauri://created") {
        listener({ payload: null });
      }
      return () => undefined;
    }
  },
}));

beforeEach(() => {
  Object.defineProperty(window, "__TAURI_INTERNALS__", {
    configurable: true,
    value: {},
  });
  tauriMocks.constructedWindows.length = 0;
  tauriMocks.cursorPosition.mockResolvedValue({ x: 900, y: 300 });
  tauriMocks.getAllWebviewWindows.mockResolvedValue([
    windowBounds("main", 0, 0, 800, 600),
    windowBounds("twominal-child", 800, 0, 800, 600),
  ]);
});

afterEach(() => {
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  vi.clearAllMocks();
});

describe("windowRuntime drag transfers", () => {
  it("disables native file-drop interception on created terminal windows", async () => {
    await createTransferWindow({
      transferToken: "transfer-token",
      requestId: "request-1",
      sourceWindowLabel: "main",
      targetWindowLabel: "twominal-request-1",
      title: "zsh",
    });

    expect(tauriMocks.constructedWindows).toEqual([
      {
        label: "twominal-request-1",
        options: expect.objectContaining({
          dragDropEnabled: false,
          url: expect.stringContaining("transferToken=transfer-token"),
        }),
      },
    ]);
  });

  it("emits a destination-bound drop request back to the source window", async () => {
    const request = {
      dragId: "drag-1",
      tabId: "tab-2",
      sourceWindowLabel: "main",
      targetWindowLabel: "twominal-child",
      toIndex: 1,
    };

    await requestTabTransfer("main", request);

    expect(tauriMocks.emitTo).toHaveBeenCalledWith(
      "main",
      "twominal-tab-transfer-request",
      request,
    );
  });

  it("detaches only when the cursor is outside every readable Twominal window", async () => {
    expect(await isCursorOutsideTwominalWindows()).toBe(false);

    tauriMocks.cursorPosition.mockResolvedValueOnce({ x: 1700, y: 700 });
    expect(await isCursorOutsideTwominalWindows()).toBe(true);

    tauriMocks.cursorPosition.mockResolvedValueOnce({ x: 1700, y: 700 });
    tauriMocks.getAllWebviewWindows.mockResolvedValueOnce([
      windowBounds("main", 0, 0, 800, 600),
      {
        ...windowBounds("twominal-child", 800, 0, 800, 600),
        outerPosition: vi.fn().mockRejectedValue(new Error("closed")),
      },
    ]);
    expect(await isCursorOutsideTwominalWindows()).toBe(false);
  });
});

function windowBounds(
  label: string,
  x: number,
  y: number,
  width: number,
  height: number,
) {
  return {
    label,
    outerPosition: vi.fn().mockResolvedValue({ x, y }),
    outerSize: vi.fn().mockResolvedValue({ width, height }),
  };
}

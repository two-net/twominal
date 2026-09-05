import type { UnlistenFn } from "@tauri-apps/api/event";
import { emitTo } from "@tauri-apps/api/event";
import { cursorPosition } from "@tauri-apps/api/window";
import {
  getAllWebviewWindows,
  getCurrentWebviewWindow,
  WebviewWindow,
} from "@tauri-apps/api/webviewWindow";

const TRANSFER_READY_EVENT = "twominal-tab-transfer-ready";
const TRANSFER_RESULT_EVENT = "twominal-tab-transfer-result";
const TRANSFER_REQUEST_EVENT = "twominal-tab-transfer-request";
const TRANSFER_TIMEOUT_MS = 10_000;

export interface TransferBootstrap {
  transferToken: string;
  requestId: string;
  sourceWindowLabel: string;
  title: string;
}

export type IncomingTabTransfer = TransferBootstrap & { toIndex?: number };

export interface TransferResult {
  requestId: string;
  ok: boolean;
  message?: string;
}

export interface TabTransferRequest {
  dragId: string;
  tabId: string;
  sourceWindowLabel: string;
  targetWindowLabel: string;
  toIndex: number;
}

export interface WindowContext {
  supported: boolean;
  label: string;
  bootstrap: TransferBootstrap | null;
}

export interface TransferWindowRequest extends TransferBootstrap {
  targetWindowLabel: string;
}

export function getWindowContext(): WindowContext {
  if (!isTauriRuntime()) {
    return {
      supported: false,
      label: "browser",
      bootstrap: null,
    };
  }

  const label = getCurrentWebviewWindow().label;
  const params = new URLSearchParams(window.location.search);
  const transferToken = params.get("transferToken");
  const requestId = params.get("transferRequest");
  const sourceWindowLabel = params.get("transferSource");
  const title = params.get("transferTitle");
  const bootstrap =
    transferToken &&
    requestId &&
    sourceWindowLabel &&
    title
      ? {
          transferToken,
          requestId,
          sourceWindowLabel,
          title,
        }
      : null;

  return {
    supported: true,
    label,
    bootstrap,
  };
}

export function createTransferIdentity(): {
  requestId: string;
  targetWindowLabel: string;
} {
  const id = randomId();
  return {
    requestId: id,
    targetWindowLabel: `twominal-${id}`,
  };
}

export async function createTransferWindow(
  request: TransferWindowRequest,
): Promise<void> {
  const query = new URLSearchParams({
    transferToken: request.transferToken,
    transferRequest: request.requestId,
    transferSource: request.sourceWindowLabel,
    transferTitle: request.title,
  });

  await new Promise<void>((resolve, reject) => {
    const child = new WebviewWindow(request.targetWindowLabel, {
      url: `index.html?${query.toString()}`,
      title: `Twominal — ${request.title}`,
      width: 800,
      height: 600,
      minWidth: 560,
      minHeight: 360,
      resizable: true,
      focus: true,
      dragDropEnabled: false,
    });
    void child.once("tauri://created", () => resolve());
    void child.once<unknown>("tauri://error", (event) => {
      reject(
        new Error(
          eventMessage(event.payload, "Unable to open a new window."),
        ),
      );
    });
  });
}

export async function listenForIncomingTransfers(
  listener: (transfer: IncomingTabTransfer) => void,
): Promise<UnlistenFn> {
  if (!isTauriRuntime()) {
    return () => undefined;
  }
  return getCurrentWebviewWindow().listen<IncomingTabTransfer>(
    TRANSFER_READY_EVENT,
    (event) => listener(event.payload),
  );
}

export async function sendIncomingTransfer(
  targetWindowLabel: string,
  transfer: IncomingTabTransfer,
): Promise<void> {
  await emitTo(targetWindowLabel, TRANSFER_READY_EVENT, transfer);
}

export async function listenForTabTransferRequests(
  listener: (request: TabTransferRequest) => void,
): Promise<UnlistenFn> {
  if (!isTauriRuntime()) {
    return () => undefined;
  }
  return getCurrentWebviewWindow().listen<TabTransferRequest>(
    TRANSFER_REQUEST_EVENT,
    (event) => listener(event.payload),
  );
}

export async function requestTabTransfer(
  sourceWindowLabel: string,
  request: TabTransferRequest,
): Promise<void> {
  await emitTo(sourceWindowLabel, TRANSFER_REQUEST_EVENT, request);
}

export async function notifyTransferResult(
  targetWindowLabel: string,
  result: TransferResult,
): Promise<void> {
  if (!isTauriRuntime()) {
    return;
  }
  await emitTo(targetWindowLabel, TRANSFER_RESULT_EVENT, result);
}

export async function waitForTransferResult(
  requestId: string,
  signal?: AbortSignal,
): Promise<TransferResult> {
  if (!isTauriRuntime()) {
    throw new Error("Native windows are unavailable in this environment.");
  }

  const current = getCurrentWebviewWindow();
  return new Promise<TransferResult>((resolve, reject) => {
    let settled = false;
    let unlisten: UnlistenFn | null = null;
    const timer = window.setTimeout(() => {
      if (settled) return;
      settled = true;
      signal?.removeEventListener("abort", abort);
      unlisten?.();
      reject(new Error("The destination window did not accept the terminal."));
    }, TRANSFER_TIMEOUT_MS);
    const abort = () => {
      if (settled) return;
      settled = true;
      window.clearTimeout(timer);
      unlisten?.();
      reject(
        new DOMException("The terminal transfer was canceled.", "AbortError"),
      );
    };
    signal?.addEventListener("abort", abort, { once: true });

    void current
      .listen<TransferResult>(TRANSFER_RESULT_EVENT, (event) => {
        if (settled || event.payload.requestId !== requestId) return;
        settled = true;
        window.clearTimeout(timer);
        signal?.removeEventListener("abort", abort);
        unlisten?.();
        resolve(event.payload);
      })
      .then((stop) => {
        if (settled) {
          stop();
        } else {
          unlisten = stop;
        }
      })
      .catch((error: unknown) => {
        if (settled) return;
        settled = true;
        window.clearTimeout(timer);
        signal?.removeEventListener("abort", abort);
        reject(error);
      });
  });
}

export async function closeCurrentWindow(): Promise<void> {
  if (isTauriRuntime()) {
    await getCurrentWebviewWindow().close();
  }
}

export async function isCursorOutsideTwominalWindows(): Promise<boolean> {
  if (!isTauriRuntime()) return false;

  const [pointer, windows] = await Promise.all([
    cursorPosition(),
    getAllWebviewWindows(),
  ]);
  const twominalWindows = windows.filter(
    (candidate) =>
      candidate.label === "main" || candidate.label.startsWith("twominal-"),
  );
  const bounds = await Promise.all(
    twominalWindows.map((candidate) =>
      Promise.all([candidate.outerPosition(), candidate.outerSize()])
        .then(([position, size]) => ({ position, size }))
        .catch(() => null),
    ),
  );
  const availableBounds = bounds.filter(
    (entry): entry is Exclude<(typeof bounds)[number], null> => entry !== null,
  );

  return (
    availableBounds.length > 0 &&
    availableBounds.length === twominalWindows.length &&
    availableBounds.every(
      ({ position, size }) =>
        pointer.x < position.x ||
        pointer.y < position.y ||
        pointer.x >= position.x + size.width ||
        pointer.y >= position.y + size.height,
    )
  );
}

function isTauriRuntime(): boolean {
  return "__TAURI_INTERNALS__" in window;
}

function randomId(): string {
  if (typeof crypto.randomUUID === "function") {
    return crypto.randomUUID();
  }
  return `${Date.now()}-${Math.random().toString(16).slice(2)}`;
}

function eventMessage(value: unknown, fallback: string): string {
  if (typeof value === "string" && value.trim()) return value;
  if (typeof value === "object" && value !== null && "message" in value) {
    const message = value.message;
    if (typeof message === "string" && message.trim()) return message;
  }
  return fallback;
}

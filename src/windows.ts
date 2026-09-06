import { PhysicalPosition } from "@tauri-apps/api/dpi";
import { emitTo } from "@tauri-apps/api/event";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import { cursorPosition, getAllWindows, getCurrentWindow } from "@tauri-apps/api/window";

export const TAB_MIME = "application/x-twominal-tab";

export type TabDrag = { id: string; from: string };

export function isTauri(): boolean {
  return "__TAURI_INTERNALS__" in window;
}

export function windowLabel(): string {
  if (!isTauri()) return "main";
  return getCurrentWindow().label;
}

export function isTabDrag(dt: DataTransfer | null): boolean {
  if (!dt) return false;
  return [...dt.types].includes(TAB_MIME);
}

export function parseTabDrag(dt: DataTransfer | null): TabDrag | null {
  if (!dt) return null;
  const raw = dt.getData(TAB_MIME) || dt.getData("text/plain");
  if (!raw) return null;
  try {
    const v = JSON.parse(raw) as TabDrag;
    if (v && typeof v.id === "string") return { id: v.id, from: typeof v.from === "string" ? v.from : "" };
  } catch {
    return { id: raw, from: "" };
  }
  return null;
}

export async function windowAtCursor(): Promise<string | null> {
  const cursor = await cursorPosition();
  return windowAtPoint(cursor.x, cursor.y);
}

export async function windowAtPoint(physX: number, physY: number): Promise<string | null> {
  const me = getCurrentWindow().label;
  const windows = await getAllWindows();
  let self = false;
  let other: string | null = null;
  for (const w of windows) {
    try {
      const pos = await w.outerPosition();
      const size = await w.outerSize();
      const inside =
        physX >= pos.x && physX <= pos.x + size.width && physY >= pos.y && physY <= pos.y + size.height;
      if (!inside) continue;
      if (w.label === me) self = true;
      else other = w.label;
    } catch {
      /* window may have closed */
    }
  }
  if (other) return other;
  if (self) return me;
  return null;
}

export async function openDetachedWindow(sessionId: string): Promise<void> {
  const current = getCurrentWindow();
  const scale = await current.scaleFactor();
  const inner = await current.innerSize();
  const logicalSize = inner.toLogical(scale);
  const cursor = await cursorPosition();
  const logical = cursor.toLogical(scale);
  const label = `tab-${crypto.randomUUID()}`;
  await new Promise<void>((resolve, reject) => {
    const child = new WebviewWindow(label, {
      url: `index.html?attach=${encodeURIComponent(sessionId)}`,
      title: "Twominal",
      width: Math.max(720, logicalSize.width),
      height: Math.max(420, logicalSize.height),
      minWidth: 720,
      minHeight: 420,
      x: Math.round(logical.x - 72),
      y: Math.round(logical.y - 18),
      decorations: true,
      focus: true,
      visible: true,
      preventOverflow: true,
      dragDropEnabled: false,
    });
    const timer = window.setTimeout(() => reject(new Error("window create timeout")), 8000);
    child.once("tauri://created", () => {
      window.clearTimeout(timer);
      resolve();
    });
    child.once("tauri://error", (e) => {
      window.clearTimeout(timer);
      reject(e.payload ?? e);
    });
  });
}

export async function moveCurrentWindowToCursor(): Promise<void> {
  const cursor = await cursorPosition();
  await getCurrentWindow().setPosition(new PhysicalPosition(cursor.x - 72, cursor.y - 18));
}

export async function emitTabMerge(label: string, id: string): Promise<void> {
  await emitTo(label, "tab-merge", { id });
  try {
    const target = await WebviewWindow.getByLabel(label);
    await target?.setFocus();
  } catch {
    /* focus is best-effort */
  }
}

export async function closeCurrentWindow(): Promise<void> {
  await getCurrentWindow().close();
}

export async function windowCount(): Promise<number> {
  return (await getAllWindows()).length;
}

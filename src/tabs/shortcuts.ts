export type TabShortcut =
  | { type: "new" }
  | { type: "close" }
  | { type: "activateIndex"; index: number }
  | { type: "previous" }
  | { type: "next" };

export interface TabShortcutEvent {
  code: string;
  altKey: boolean;
  ctrlKey: boolean;
  metaKey: boolean;
  shiftKey: boolean;
  isComposing: boolean;
  repeat: boolean;
}

export function tabShortcutFor(
  event: TabShortcutEvent,
  applePlatform: boolean,
): TabShortcut | null {
  if (event.isComposing || event.repeat) {
    return null;
  }

  const usesCommandModifier = applePlatform
    ? event.metaKey && !event.ctrlKey
    : event.ctrlKey && !event.metaKey;
  if (!usesCommandModifier || event.altKey) {
    return null;
  }

  if (!event.shiftKey && event.code === "KeyT") {
    return { type: "new" };
  }
  if (!event.shiftKey && event.code === "KeyW") {
    return { type: "close" };
  }
  if (!event.shiftKey && /^Digit[1-9]$/.test(event.code)) {
    return {
      type: "activateIndex",
      index: Number(event.code.slice(-1)) - 1,
    };
  }
  if (event.shiftKey && event.code === "BracketLeft") {
    return { type: "previous" };
  }
  if (event.shiftKey && event.code === "BracketRight") {
    return { type: "next" };
  }
  return null;
}

export function isApplePlatform(platform: string): boolean {
  return /Mac|iPhone|iPad|iPod/i.test(platform);
}

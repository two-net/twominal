import { Channel, invoke } from "@tauri-apps/api/core";
import type {
  SessionDescriptor,
  SessionLifecycleEvent,
  TerminalSize,
} from "./types";

const SESSION_HEADER = "X-Twominal-Session-Id";

export interface TerminalClient {
  start(
    size: TerminalSize,
    onOutput: (data: ArrayBuffer) => void,
    onLifecycle: (event: SessionLifecycleEvent) => void,
  ): Promise<SessionDescriptor>;
  attach(
    transferToken: string,
    size: TerminalSize,
    onSnapshot: (data: ArrayBuffer) => void,
    onOutput: (data: ArrayBuffer) => void,
    onLifecycle: (event: SessionLifecycleEvent) => void,
  ): Promise<SessionDescriptor>;
  write(sessionId: string, data: Uint8Array): Promise<void>;
  resize(sessionId: string, size: TerminalSize): Promise<void>;
  acknowledgeOutput(sessionId: string, bytes: number): Promise<void>;
  close(sessionId: string): Promise<void>;
}

export class TauriTerminalClient implements TerminalClient {
  async start(
    size: TerminalSize,
    onOutput: (data: ArrayBuffer) => void,
    onLifecycle: (event: SessionLifecycleEvent) => void,
  ): Promise<SessionDescriptor> {
    const output = new Channel<ArrayBuffer>(onOutput);
    const lifecycle = new Channel<SessionLifecycleEvent>(onLifecycle);

    return invoke<SessionDescriptor>("terminal_start", {
      size,
      output,
      lifecycle,
    });
  }

  async attach(
    transferToken: string,
    size: TerminalSize,
    onSnapshot: (data: ArrayBuffer) => void,
    onOutput: (data: ArrayBuffer) => void,
    onLifecycle: (event: SessionLifecycleEvent) => void,
  ): Promise<SessionDescriptor> {
    const snapshot = new Channel<ArrayBuffer>(onSnapshot);
    const output = new Channel<ArrayBuffer>(onOutput);
    const lifecycle = new Channel<SessionLifecycleEvent>(onLifecycle);

    return invoke<SessionDescriptor>("terminal_attach_transfer", {
      transferToken,
      size,
      snapshot,
      output,
      lifecycle,
    });
  }

  async write(sessionId: string, data: Uint8Array): Promise<void> {
    await invoke<void>("terminal_write", data, {
      headers: {
        [SESSION_HEADER]: sessionId,
      },
    });
  }

  async resize(sessionId: string, size: TerminalSize): Promise<void> {
    await invoke<void>("terminal_resize", { sessionId, size });
  }

  async acknowledgeOutput(sessionId: string, bytes: number): Promise<void> {
    await invoke<void>("terminal_ack_output", { sessionId, bytes });
  }

  async close(sessionId: string): Promise<void> {
    await invoke<void>("terminal_close", { sessionId });
  }
}

export async function prepareTerminalTransfer(
  sessionId: string,
  targetWindowLabel: string,
): Promise<string> {
  return invoke<string>("terminal_prepare_transfer", {
    sessionId,
    targetWindowLabel,
  });
}

export async function cancelTerminalTransfer(
  transferToken: string,
): Promise<void> {
  await invoke<void>("terminal_cancel_transfer", { transferToken });
}

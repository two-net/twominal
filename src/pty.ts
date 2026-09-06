import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

type OutputHandler = (bytes: Uint8Array) => void;
type ExitHandler = () => void;

const outputs = new Map<string, OutputHandler>();
const exits = new Map<string, ExitHandler>();
let started = false;
let unlistens: UnlistenFn[] = [];

function b64ToBytes(s: string): Uint8Array {
  const bin = atob(s);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

export async function initPtyEvents(): Promise<void> {
  if (started) return;
  started = true;
  unlistens.push(
    await listen<{ id: string; data: string }>("pty-output", (ev) => {
      outputs.get(ev.payload.id)?.(b64ToBytes(ev.payload.data));
    }),
  );
  unlistens.push(
    await listen<{ id: string }>("pty-exit", (ev) => {
      exits.get(ev.payload.id)?.();
    }),
  );
}

export function onPtyOutput(id: string, fn: OutputHandler): void {
  outputs.set(id, fn);
}

export function onPtyExit(id: string, fn: ExitHandler): void {
  exits.set(id, fn);
}

export function dropPtyHandlers(id: string): void {
  outputs.delete(id);
  exits.delete(id);
}

export function ptySpawn(cols: number, rows: number, shell: string | null, cwd: string | null): Promise<string> {
  return invoke<string>("pty_spawn", { cols, rows, shell, cwd });
}

export function ptyWrite(id: string, data: string): Promise<void> {
  return invoke("pty_write", { id, data });
}

export function ptyResize(id: string, cols: number, rows: number): Promise<void> {
  return invoke("pty_resize", { id, cols, rows });
}

export function ptyKill(id: string): Promise<void> {
  return invoke("pty_kill", { id });
}

export function ptyPark(id: string, snapshot: string): Promise<void> {
  return invoke("pty_park", { id, snapshot });
}

export async function ptyUnpark(id: string): Promise<{ snapshot: string; backlog: Uint8Array; exited: boolean }> {
  const parked = await invoke<{ snapshot: string; backlog: string; exited?: boolean }>("pty_unpark", { id });
  return {
    snapshot: parked.snapshot ?? "",
    backlog: parked.backlog ? b64ToBytes(parked.backlog) : new Uint8Array(),
    exited: !!parked.exited,
  };
}

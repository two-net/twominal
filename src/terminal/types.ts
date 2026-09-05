export interface TerminalSize {
  rows: number;
  cols: number;
}

export interface SessionDescriptor {
  sessionId: string;
  shellName: string;
  cwd: string | null;
  shellIntegration: boolean;
  shellIntegrationNonce: string | null;
}

export type SessionLifecycleEvent =
  | {
      type: "exited";
      exitCode: number;
      signal: string | null;
    }
  | {
      type: "error";
      code: string;
      message: string;
    };

export type TerminalViewState =
  | { type: "starting" }
  | { type: "running"; session: SessionDescriptor }
  | {
      type: "exited";
      session: SessionDescriptor;
      exitCode: number;
      signal: string | null;
    }
  | { type: "error"; message: string };

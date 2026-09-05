import { StrictMode } from "react";
import { cleanup, render, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { TerminalPane } from "./TerminalPane";

const controllerMocks = vi.hoisted(() => ({
  constructors: vi.fn(),
  start: vi.fn().mockResolvedValue(undefined),
  dispose: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("./TerminalController", () => ({
  TerminalController: class {
    constructor(host: HTMLElement, options: unknown) {
      controllerMocks.constructors(host, options);
    }

    start = controllerMocks.start;
    dispose = controllerMocks.dispose;
    applyAppearance = vi.fn();
    applyTerminalConfig = vi.fn();
    applyVimMode = vi.fn();
    updateHistory = vi.fn();
    setActive = vi.fn();
  },
}));

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("TerminalPane", () => {
  it("reuses a transferred controller across the Strict Mode effect probe", async () => {
    const view = render(
      <StrictMode>
        <TerminalPane
          appearance="dark"
          active
          terminalConfig={{
            fontFamily: "monospace",
            fontSize: 14,
            lineHeight: 1.2,
            letterSpacing: 0,
            fontWeight: 400,
            fontLigatures: true,
          }}
          vimMode={false}
          restartKey={0}
          transferToken="one-time-token"
          history={[]}
          onCommandAccepted={vi.fn()}
          onShellExperienceChange={vi.fn()}
          onStateChange={vi.fn()}
          onTitleChange={vi.fn()}
        />
      </StrictMode>,
    );

    expect(controllerMocks.constructors).toHaveBeenCalledOnce();
    expect(controllerMocks.constructors).toHaveBeenCalledWith(
      expect.any(HTMLElement),
      expect.objectContaining({ transferToken: "one-time-token" }),
    );
    expect(controllerMocks.start).toHaveBeenCalledOnce();
    expect(controllerMocks.dispose).not.toHaveBeenCalled();

    view.unmount();
    await waitFor(() => expect(controllerMocks.dispose).toHaveBeenCalledOnce());
  });
});

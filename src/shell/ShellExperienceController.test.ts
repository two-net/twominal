import type { Terminal } from "@xterm/xterm";
import { describe, expect, it, vi } from "vitest";
import type { ShellClient } from "./shellClient";
import {
  ShellExperienceController,
  parseCwdProperty,
  type ShellExperienceStatus,
} from "./ShellExperienceController";
import type { HistoryEntry } from "./history";

describe("parseCwdProperty", () => {
  it("decodes authenticated UTF-8 working directories", () => {
    const encoded = Array.from(new TextEncoder().encode("/tmp/โปรเจกต์"))
      .map((byte) => byte.toString(16).padStart(2, "0"))
      .join("");
    expect(parseCwdProperty(`P;CwdHex=${encoded};nonce`, "nonce")).toBe(
      "/tmp/โปรเจกต์",
    );
  });

  it("rejects a wrong nonce, malformed hex, and control-bearing paths", () => {
    expect(parseCwdProperty("P;CwdHex=2f746d70;wrong", "nonce")).toBeNull();
    expect(parseCwdProperty("P;CwdHex=xyz;nonce", "nonce")).toBeNull();
    expect(parseCwdProperty("P;CwdHex=2f746d700a;nonce", "nonce")).toBeNull();
  });
});

describe("ShellExperienceController", () => {
  it("reports a working directory only after supported integration is configured", () => {
    const harness = createController(createShellClient([]));

    expect(harness.statuses[0]).toMatchObject({
      phase: "unavailable",
      cwd: null,
    });
    expect(harness.statuses.at(-1)).toMatchObject({
      phase: "waiting",
      cwd: "/tmp",
    });
    harness.controller.dispose();
  });

  it("publishes an authenticated CwdHex marker immediately", () => {
    const harness = createController(createShellClient([]));
    const statusCount = harness.statuses.length;

    harness.marker("P;CwdHex=2f776f726b7370616365;nonce");

    expect(harness.statuses).toHaveLength(statusCount + 1);
    expect(harness.statuses.at(-1)?.cwd).toBe("/workspace");
    harness.controller.dispose();
  });

  it("ignores unauthenticated and malformed working-directory markers", () => {
    const harness = createController(createShellClient([]));
    const statusCount = harness.statuses.length;

    harness.marker("P;CwdHex=2f736563726574;wrong");
    harness.marker("P;CwdHex=xyz;nonce");

    expect(harness.statuses).toHaveLength(statusCount);
    expect(harness.statuses.at(-1)?.cwd).toBe("/tmp");
    harness.controller.dispose();
  });

  it("does not expose a backend cwd without supported shell integration", () => {
    const harness = createController(createShellClient([]), {
      shellIntegration: false,
    });

    expect(harness.statuses.at(-1)).toMatchObject({
      phase: "unavailable",
      cwd: null,
    });
    harness.marker("P;CwdHex=2f736563726574;nonce");
    expect(harness.statuses.at(-1)?.cwd).toBeNull();
    harness.controller.dispose();
  });

  it("resolves an early Tab immediately and inserts a unique completion", async () => {
    const shellClient = createShellClient([
      { value: "whoami", display: "whoami", kind: "executable" },
    ]);
    const harness = createController(shellClient);

    harness.enterPrompt();
    harness.input("whoam");
    expect(harness.key(new KeyboardEvent("keydown", { key: "Tab" }))).toBe(
      false,
    );

    await vi.waitFor(() => expect(harness.sent).toEqual(["i "]));
    expect(shellClient.complete).toHaveBeenCalledOnce();
    harness.controller.dispose();
  });

  it.each([
    ["command", "gi", "git", "executable", "t "],
    ["path", "cat src/ter", "src/terminal/", "directory", "minal/"],
  ] as const)(
    "automatically suggests a %s completion without command history",
    async (_label, input, value, kind, suffix) => {
      const harness = createController(
        createShellClient([{ value, display: value, kind }]),
      );

      harness.enterPrompt();
      harness.input(input);

      await vi.waitFor(() => {
        expect(
          harness.host.querySelector(".terminal-autosuggestion")?.textContent,
        ).toBe(suffix);
        expect(harness.statuses.at(-1)?.suggestionAvailable).toBe(true);
      });

      expect(
        harness.key(new KeyboardEvent("keydown", { key: "ArrowRight" })),
      ).toBe(false);
      expect(harness.sent).toEqual([suffix]);
      harness.controller.dispose();
    },
  );

  it("keeps a matching history autosuggestion ahead of native completions", async () => {
    const harness = createController(
      createShellClient([
        { value: "git", display: "git", kind: "executable" },
      ]),
      {
        history: [
          {
            command: "git status",
            lastUsedSequence: 1,
            frequency: 1,
          },
        ],
      },
    );

    harness.enterPrompt();
    harness.input("gi");

    await vi.waitFor(() =>
      expect(harness.statuses.at(-1)?.completionCount).toBe(1),
    );
    expect(
      harness.host.querySelector(".terminal-autosuggestion")?.textContent,
    ).toBe("t status");
    harness.controller.dispose();
  });

  it("forwards Tab to the shell when Twominal has no candidates", async () => {
    const shellClient = createShellClient([]);
    const harness = createController(shellClient);

    harness.enterPrompt();
    harness.input("alias-command");
    expect(harness.key(new KeyboardEvent("keydown", { key: "Tab" }))).toBe(
      false,
    );

    await vi.waitFor(() => expect(harness.sent).toEqual(["\t"]));
    harness.controller.dispose();
  });

  it("runs Vim editing at an authenticated prompt and reports its mode", () => {
    const harness = createController(createShellClient([]), { vimMode: true });

    expect(harness.key(new KeyboardEvent("keydown", { key: "Escape" }))).toBe(
      true,
    );
    harness.enterPrompt();
    expect(harness.statuses.at(-1)?.inputMode).toBe("insert");

    harness.input("a👩🏽‍💻b");
    expect(harness.key(new KeyboardEvent("keydown", { key: "Escape" }))).toBe(
      false,
    );
    expect(harness.statuses.at(-1)?.inputMode).toBe("normal");
    expect(harness.sent.at(-1)).toBe("\u001b[D");

    expect(harness.key(new KeyboardEvent("keydown", { key: "x" }))).toBe(
      false,
    );
    expect(harness.sent).toContain("\u007f".repeat(6));
    expect(harness.key(new KeyboardEvent("keydown", { key: "i" }))).toBe(
      false,
    );
    expect(harness.statuses.at(-1)?.inputMode).toBe("insert");
    harness.controller.dispose();
  });

  it("uses PSReadLine's UTF-16 edit units for non-BMP text", () => {
    const harness = createController(createShellClient([]), {
      vimMode: true,
      shellName: "pwsh",
    });
    harness.enterPrompt();
    harness.key(new KeyboardEvent("keydown", { key: "i" }));
    harness.input("a😀b");
    harness.key(new KeyboardEvent("keydown", { key: "Escape" }));
    harness.key(new KeyboardEvent("keydown", { key: "h" }));

    expect(harness.sent.at(-1)).toBe("\u001b[D".repeat(2));
    harness.key(new KeyboardEvent("keydown", { key: "x" }));
    expect(harness.sent).toContain("\u007f".repeat(4));
    harness.controller.dispose();
  });

  it("uses Normal-mode j and k for filtered Twominal history", () => {
    const history: HistoryEntry[] = [
      { command: "git status", lastUsedSequence: 2, frequency: 1 },
      { command: "npm test", lastUsedSequence: 1, frequency: 1 },
    ];
    const harness = createController(createShellClient([]), {
      vimMode: true,
      history,
    });
    harness.enterPrompt();
    harness.key(new KeyboardEvent("keydown", { key: "i" }));
    harness.input("git");
    harness.key(new KeyboardEvent("keydown", { key: "Escape" }));

    expect(harness.key(new KeyboardEvent("keydown", { key: "k" }))).toBe(
      false,
    );
    expect(harness.sent.join("")).toContain("git status");
    expect(harness.statuses.at(-1)?.inputMode).toBe("normal");

    expect(harness.key(new KeyboardEvent("keydown", { key: "j" }))).toBe(
      false,
    );
    harness.controller.dispose();
  });

  it("never intercepts Vim commands outside the safe prompt boundary", () => {
    const harness = createController(createShellClient([]), { vimMode: true });
    harness.enterPrompt();
    harness.key(new KeyboardEvent("keydown", { key: "i" }));
    harness.input("abc");
    harness.key(new KeyboardEvent("keydown", { key: "Escape" }));
    const sentAtPrompt = harness.sent.length;

    harness.buffer.baseY = 4;
    expect(harness.key(new KeyboardEvent("keydown", { key: "x" }))).toBe(
      false,
    );
    expect(harness.sent.length).toBeGreaterThan(sentAtPrompt);
    expect(harness.statuses.at(-1)?.inputMode).toBe("normal");
    expect(harness.buffer.viewportY).toBe(harness.buffer.baseY);

    harness.buffer.type = "alternate";
    const sentBeforeAlternateScreen = harness.sent.length;
    expect(harness.key(new KeyboardEvent("keydown", { key: "x" }))).toBe(
      true,
    );
    expect(harness.sent).toHaveLength(sentBeforeAlternateScreen);

    harness.buffer.type = "normal";
    harness.buffer.viewportY = harness.buffer.baseY;
    expect(harness.key(new KeyboardEvent("keydown", { key: "Enter" }))).toBe(
      true,
    );
    expect(harness.input("\r")).toBe(true);
    expect(harness.key(new KeyboardEvent("keydown", { key: "x" }))).toBe(
      true,
    );
    expect(harness.statuses.at(-1)?.inputMode).toBeNull();
    harness.controller.dispose();
  });

  it("blocks paste and IME-style data in Normal mode but not in programs", () => {
    const harness = createController(createShellClient([]), { vimMode: true });
    harness.enterPrompt();
    harness.key(new KeyboardEvent("keydown", { key: "i" }));
    expect(harness.input("echo")).toBe(true);
    harness.key(new KeyboardEvent("keydown", { key: "Escape" }));

    expect(
      harness.input("\u001b[200~ pasted text\u001b[201~"),
    ).toBe(false);
    expect(harness.input("入力")).toBe(false);
    expect(harness.input("\r")).toBe(false);
    expect(harness.statuses.at(-1)?.inputMode).toBe("normal");

    harness.buffer.type = "alternate";
    expect(harness.input("入力")).toBe(true);
    harness.controller.dispose();
  });

  it("passes an authenticated Normal-mode control key to the PTY", () => {
    const harness = createController(createShellClient([]), { vimMode: true });
    harness.enterPrompt();
    harness.input("sleep 10");
    harness.key(new KeyboardEvent("keydown", { key: "Escape" }));

    expect(harness.input("\u0003")).toBe(false);
    expect(
      harness.key(
        new KeyboardEvent("keydown", { key: "c", ctrlKey: true }),
      ),
    ).toBe(true);
    expect(harness.input("\u0003")).toBe(true);
    expect(harness.statuses.at(-1)?.inputMode).toBeNull();
    harness.controller.dispose();
  });

  it("applies the Vim setting immediately without restarting a session", () => {
    const harness = createController(createShellClient([]));
    harness.enterPrompt();
    expect(harness.statuses.at(-1)?.inputMode).toBeNull();

    harness.controller.setVimMode(true);
    expect(harness.statuses.at(-1)?.inputMode).toBe("insert");
    harness.controller.setVimMode(false);
    expect(harness.statuses.at(-1)?.inputMode).toBeNull();
    expect(harness.key(new KeyboardEvent("keydown", { key: "x" }))).toBe(
      true,
    );
    harness.controller.dispose();
  });

  it.each(["i", "a", "A", "I"])(
    "consumes every browser event for the Normal-mode %s transition",
    (command) => {
      const harness = createController(createShellClient([]), {
        vimMode: true,
      });
      harness.enterPrompt();
      harness.input("echo");
      harness.key(new KeyboardEvent("keydown", { key: "Escape" }));

      const transition = harness.printableStroke(command);

      expect(transition).toEqual({
        keydownAllowed: false,
        keypressAllowed: false,
        dataAllowed: false,
        keyupAllowed: true,
      });
      expect(harness.statuses.at(-1)?.inputMode).toBe("insert");
      harness.controller.dispose();
    },
  );

  it.each(["cw", "ciw"])(
    "does not leak the final character of the Normal-mode %s change command",
    (command) => {
      const harness = createController(createShellClient([]), {
        vimMode: true,
      });
      harness.enterPrompt();
      harness.input("one two");
      harness.key(new KeyboardEvent("keydown", { key: "Escape" }));

      const strokes = Array.from(command, (key) =>
        harness.printableStroke(key),
      );

      expect(strokes).toEqual(
        command.split("").map(() => ({
          keydownAllowed: false,
          keypressAllowed: false,
          dataAllowed: false,
          keyupAllowed: true,
        })),
      );
      expect(harness.statuses.at(-1)?.inputMode).toBe("insert");
      harness.controller.dispose();
    },
  );

  it("passes a fresh printable key exactly once when already in Insert mode", () => {
    const harness = createController(createShellClient([]), { vimMode: true });
    harness.enterPrompt();
    harness.key(new KeyboardEvent("keydown", { key: "i" }));

    expect(harness.printableStroke("z")).toEqual({
      keydownAllowed: true,
      keypressAllowed: true,
      dataAllowed: true,
      keyupAllowed: true,
    });
    expect(harness.statuses.at(-1)?.inputMode).toBe("insert");
    harness.controller.dispose();
  });

  it("treats an auto-repeat keydown as a new key after entering Insert", () => {
    const harness = createController(createShellClient([]), { vimMode: true });
    harness.enterPrompt();
    harness.input("echo");
    harness.key(new KeyboardEvent("keydown", { key: "Escape" }));
    const eventOptions: KeyboardEventInit = {
      key: "i",
      code: "KeyI",
      cancelable: true,
    };

    const transitionKeydown = new KeyboardEvent("keydown", eventOptions);
    expect(harness.key(transitionKeydown)).toBe(false);
    expect(transitionKeydown.defaultPrevented).toBe(true);

    const transitionKeypress = new KeyboardEvent("keypress", eventOptions);
    expect(harness.key(transitionKeypress)).toBe(false);
    expect(transitionKeypress.defaultPrevented).toBe(true);

    const repeatedKeydown = new KeyboardEvent("keydown", {
      ...eventOptions,
      repeat: true,
    });
    expect(harness.key(repeatedKeydown)).toBe(true);
    expect(repeatedKeydown.defaultPrevented).toBe(false);
    expect(harness.input("i")).toBe(true);
    harness.controller.dispose();
  });

  it("preserves Normal mode when an authenticated prompt is reactivated", () => {
    const harness = createController(createShellClient([]), { vimMode: true });
    harness.enterPrompt();
    harness.input("echo");
    harness.key(new KeyboardEvent("keydown", { key: "Escape" }));

    harness.controller.setActive(false);
    harness.controller.setActive(true);

    expect(harness.statuses.at(-1)?.inputMode).toBe("normal");
    expect(harness.printableStroke("i")).toEqual({
      keydownAllowed: false,
      keypressAllowed: false,
      dataAllowed: false,
      keyupAllowed: true,
    });
    expect(harness.statuses.at(-1)?.inputMode).toBe("insert");
    harness.controller.dispose();
  });

  it("does not change modes when reactivating a full-screen application", () => {
    const harness = createController(createShellClient([]), { vimMode: true });
    harness.enterPrompt();
    harness.input("nvim");
    harness.key(new KeyboardEvent("keydown", { key: "Escape" }));
    harness.buffer.type = "alternate";

    harness.controller.setActive(false);
    harness.controller.setActive(true);

    expect(harness.statuses.at(-1)?.inputMode).toBeNull();
    expect(harness.key(new KeyboardEvent("keydown", { key: "i" }))).toBe(
      true,
    );
    harness.controller.dispose();
  });
});

function createController(
  shellClient: ShellClient,
  options: {
    vimMode?: boolean;
    history?: readonly HistoryEntry[];
    shellName?: string;
    shellIntegration?: boolean;
  } = {},
) {
  let oscHandler: (data: string) => boolean = () => false;
  let keyHandler: (event: KeyboardEvent) => boolean = () => true;
  const disposable = () => ({ dispose: vi.fn() });
  const buffer = {
    type: "normal",
    viewportY: 0,
    baseY: 0,
    cursorX: 0,
    cursorY: 0,
  };
  const terminal = {
    parser: {
      registerOscHandler: (_identifier: number, handler: typeof oscHandler) => {
        oscHandler = handler;
        return disposable();
      },
    },
    onData: () => disposable(),
    onRender: () => disposable(),
    onScroll: () => disposable(),
    onResize: () => disposable(),
    attachCustomKeyEventHandler: (handler: typeof keyHandler) => {
      keyHandler = handler;
    },
    buffer: { active: buffer },
    cols: 80,
    rows: 24,
    element: undefined,
    focus: vi.fn(),
    scrollToBottom: vi.fn(() => {
      buffer.viewportY = buffer.baseY;
    }),
  } as unknown as Terminal;
  const sent: string[] = [];
  const statuses: ShellExperienceStatus[] = [];
  const host = document.createElement("div");
  const controller = new ShellExperienceController({
    terminal,
    host,
    client: shellClient,
    vimMode: options.vimMode,
    history: options.history,
    sendData: (data) => sent.push(data),
    onCommandAccepted: vi.fn(),
    onStatusChange: (status) => statuses.push(status),
  });
  controller.setSession({
    sessionId: "session-id",
    shellName: options.shellName ?? "zsh",
    cwd: "/tmp",
    shellIntegration: options.shellIntegration ?? true,
    shellIntegrationNonce:
      options.shellIntegration === false ? null : "nonce",
  });

  const input = (data: string) => controller.handleTerminalData(data);
  const printableStroke = (key: string) => {
    const code = /^[a-z]$/i.test(key) ? `Key${key.toUpperCase()}` : key;
    const eventOptions: KeyboardEventInit = {
      key,
      code,
      bubbles: true,
      cancelable: true,
      shiftKey: key !== key.toLowerCase(),
    };
    const keydownAllowed = keyHandler(
      new KeyboardEvent("keydown", eventOptions),
    );
    const keypressAllowed = keyHandler(
      new KeyboardEvent("keypress", eventOptions),
    );
    // xterm normally emits data only for an allowed key event. Calling the
    // boundary unconditionally also covers browser input events that can be
    // delivered independently of keypress.
    const dataAllowed = input(key);
    const keyupAllowed = keyHandler(new KeyboardEvent("keyup", eventOptions));
    return {
      keydownAllowed,
      keypressAllowed,
      dataAllowed,
      keyupAllowed,
    };
  };

  return {
    controller,
    buffer,
    host,
    sent,
    statuses,
    input,
    key: (event: KeyboardEvent) => keyHandler(event),
    printableStroke,
    marker: (data: string) => oscHandler(data),
    enterPrompt: () => {
      oscHandler("A;nonce");
      oscHandler("B;nonce");
    },
  };
}

function createShellClient(
  completions: Awaited<ReturnType<ShellClient["complete"]>>,
): ShellClient & { complete: ReturnType<typeof vi.fn<ShellClient["complete"]>> } {
  return {
    loadHistory: () => Promise.resolve([]),
    appendHistory: () => Promise.resolve(null),
    clearHistory: () => Promise.resolve(),
    complete: vi.fn(() => Promise.resolve(completions)),
  };
}

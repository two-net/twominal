import { describe, expect, it } from "vitest";
import {
  PromptInputModel,
  countGraphemes,
  isGraphemeBoundary,
} from "./PromptInputModel";

const NONCE = "trusted-session";

describe("PromptInputModel", () => {
  it("only enters editing for authenticated prompt markers", () => {
    const model = new PromptInputModel(NONCE);
    model.handleOsc133("B;other-session");
    expect(model.snapshot().phase).toBe("inactive");

    model.handleOsc133(`A;${NONCE}`);
    model.handleOsc133(`B;${NONCE}`);
    expect(model.snapshot()).toMatchObject({
      phase: "editing",
      reliable: true,
      line: "",
      cursor: 0,
    });
  });

  it("tracks insertion, Unicode-aware editing, and common readline movements", () => {
    const model = editingModel();
    model.handleData("echo 👩🏽‍💻");
    model.handleData("\u007f");
    expect(model.snapshot().line).toBe("echo ");

    model.handleData("status");
    model.handleData("\u0001");
    model.handleData("X");
    model.handleData("\u0005");
    model.handleData("!");
    expect(model.snapshot().line).toBe("Xecho status!");
  });

  it("commits a safe command only when the shell confirms execution", () => {
    const model = editingModel();
    model.handleData("git status");
    expect(model.handleData("\r").committedCommand).toBeUndefined();
    expect(model.snapshot().phase).toBe("inactive");

    expect(model.handleOsc133(`C;${NONCE}`).committedCommand).toBe(
      "git status",
    );
    expect(model.handleOsc133(`A;${NONCE}`).committedCommand).toBeUndefined();
  });

  it("drops pending history when a program requests more input", () => {
    const model = editingModel();
    model.handleData("sudo command");
    model.handleData("\r");
    model.handleData("secret input");
    expect(model.handleOsc133(`A;${NONCE}`).committedCommand).toBeUndefined();
  });

  it("returns to reliable prompt editing after Ctrl+C", () => {
    const model = editingModel();
    model.handleData("unfinished");
    model.handleData("\u0003");
    expect(model.snapshot()).toMatchObject({
      phase: "inactive",
      reliable: false,
      line: "",
    });

    model.handleOsc133(`D;;${NONCE}`);
    model.handleOsc133(`A;${NONCE}`);
    model.handleOsc133(`B;${NONCE}`);
    expect(model.snapshot()).toMatchObject({
      phase: "editing",
      reliable: true,
      line: "",
      cursor: 0,
    });
  });

  it("does not retain private commands or unreliable edited input", () => {
    const privateModel = editingModel();
    privateModel.handleData(" secret");
    privateModel.handleData("\r");
    expect(
      privateModel.handleOsc133(`C;${NONCE}`).committedCommand,
    ).toBeUndefined();

    const unreliableModel = editingModel();
    unreliableModel.handleData("echo value");
    unreliableModel.handleData("\u001b[9~");
    expect(unreliableModel.snapshot().reliable).toBe(false);
    unreliableModel.handleData("\r");
    expect(
      unreliableModel.handleOsc133(`C;${NONCE}`).committedCommand,
    ).toBeUndefined();
  });

  it("tracks safe bracketed paste without retaining multiline paste", () => {
    const model = editingModel();
    model.handleData("\u001b[200~hello world\u001b[201~");
    expect(model.snapshot().line).toBe("hello world");

    model.handleData("\u001b[200~\nsecret\u001b[201~");
    expect(model.snapshot().reliable).toBe(false);
  });

  it("accepts synthetic replacements at a grapheme boundary", () => {
    const model = editingModel();
    model.replaceLine("a👩🏽‍💻b", 1);

    expect(model.snapshot()).toMatchObject({
      line: "a👩🏽‍💻b",
      cursor: 1,
      reliable: true,
    });

    model.replaceLine("a👩🏽‍💻b", 2);
    expect(model.snapshot().reliable).toBe(false);
  });
});

it("counts extended grapheme clusters for safe line replacement", () => {
  expect(countGraphemes("a👩🏽‍💻é")).toBe(3);
  expect(isGraphemeBoundary("a👩🏽‍💻b", 1)).toBe(true);
  expect(isGraphemeBoundary("a👩🏽‍💻b", 2)).toBe(false);
});

function editingModel(): PromptInputModel {
  const model = new PromptInputModel(NONCE);
  model.handleOsc133(`A;${NONCE}`);
  model.handleOsc133(`B;${NONCE}`);
  return model;
}

import { describe, expect, it } from "vitest";
import {
  chunkBytes,
  concatBytes,
  encodeBinaryData,
  encodeTerminalData,
} from "./bytes";

describe("terminal byte encoding", () => {
  it("encodes terminal input as UTF-8", () => {
    expect(Array.from(encodeTerminalData("A界🙂"))).toEqual([
      65, 231, 149, 140, 240, 159, 153, 130,
    ]);
  });

  it("preserves the low byte of xterm binary strings", () => {
    expect(Array.from(encodeBinaryData("\u0000\u001b\u00ff\u1234"))).toEqual([
      0, 27, 255, 52,
    ]);
  });

  it("chunks large paste data without losing order", () => {
    const source = Uint8Array.from({ length: 11 }, (_, index) => index);
    const chunks = chunkBytes(source, 4);
    expect(chunks.map((chunk) => chunk.byteLength)).toEqual([4, 4, 3]);
    expect(Array.from(chunks.reduce(concatBytes))).toEqual(Array.from(source));
  });

  it("rejects an invalid maximum chunk size", () => {
    expect(() => chunkBytes(new Uint8Array([1]), 0)).toThrow(RangeError);
  });
});

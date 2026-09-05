import { describe, expect, it } from "vitest";
import { findLigatureRanges } from "./ligatures";

describe("findLigatureRanges", () => {
  it("joins common programming ligatures", () => {
    expect(findLigatureRanges("left !== right => next")).toEqual([
      [5, 8],
      [15, 17],
    ]);
  });

  it("prefers the longest match and emits non-overlapping ranges", () => {
    expect(findLigatureRanges("<====> !== ==")).toEqual([
      [0, 6],
      [7, 10],
      [11, 13],
    ]);
  });

  it("does not join ordinary terminal text", () => {
    expect(findLigatureRanges("git status /tmp/file.txt")).toEqual([]);
  });
});

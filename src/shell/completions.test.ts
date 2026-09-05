import { describe, expect, it } from "vitest";
import {
  buildCompletionChoices,
  completionRequestForLine,
  type NativeCompletionCandidate,
} from "./completions";

describe("completionRequestForLine", () => {
  it("routes commands, paths, and environment references to modular sources", () => {
    expect(completionRequestForLine("git", 3, "/tmp")).toEqual({
      kind: "executable",
      prefix: "git",
      cwd: "/tmp",
    });
    expect(completionRequestForLine("cat src/ter", 11, "/tmp")).toEqual({
      kind: "path",
      prefix: "src/ter",
      cwd: "/tmp",
    });
    expect(completionRequestForLine("echo $PA", 8, "/tmp")).toEqual({
      kind: "environment",
      prefix: "PA",
      cwd: "/tmp",
    });
    expect(completionRequestForLine("echo $env:PA", 12, "/tmp")).toEqual({
      kind: "environment",
      prefix: "PA",
      cwd: "/tmp",
    });
    expect(completionRequestForLine("echo %PA", 8, "/tmp")).toEqual({
      kind: "environment",
      prefix: "PA",
      cwd: "/tmp",
    });
  });

  it("does not complete options, unfinished quotes, or text away from the cursor", () => {
    expect(completionRequestForLine("git --ver", 9, null)).toBeNull();
    expect(completionRequestForLine('echo "src', 9, null)).toBeNull();
    expect(completionRequestForLine("git status", 3, null)).toBeNull();
  });
});

describe("buildCompletionChoices", () => {
  it("produces suffix-only safe insertions and keeps directories open", () => {
    const candidates: NativeCompletionCandidate[] = [
      candidate("src/terminal/", "directory"),
      candidate("src/terminal file.ts", "file"),
    ];
    expect(buildCompletionChoices("cat src/ter", 11, candidates)).toEqual([
      expect.objectContaining({ insertion: "minal/" }),
      expect.objectContaining({ insertion: "minal\\ file.ts " }),
    ]);
  });

  it("extends a closed quoted word without creating an unquoted space", () => {
    expect(
      buildCompletionChoices('cat "my fi"', 11, [
        candidate("my file copy.txt", "file"),
      ])[0].insertion,
    ).toBe("le\\ copy.txt ");
  });

  it("deduplicates and rejects control-bearing or unrelated candidates", () => {
    const candidates = [
      candidate("git", "executable"),
      candidate("git", "executable"),
      candidate("gzip", "executable"),
      candidate("gi\nunsafe", "executable"),
      candidate("gi\u202eunsafe", "executable"),
    ];
    expect(buildCompletionChoices("gi", 2, candidates)).toEqual([
      expect.objectContaining({ value: "git", insertion: "t " }),
    ]);
  });

  it("preserves POSIX, PowerShell, braced, and cmd environment syntax", () => {
    const environment = candidate("PATH", "environment");
    expect(
      buildCompletionChoices("echo $PA", 8, [environment])[0].insertion,
    ).toBe("TH ");
    expect(
      buildCompletionChoices("echo $env:PA", 12, [environment])[0].insertion,
    ).toBe("TH ");
    expect(
      buildCompletionChoices("echo ${PA", 9, [environment])[0].insertion,
    ).toBe("TH} ");
    expect(
      buildCompletionChoices("echo %PA", 8, [environment])[0].insertion,
    ).toBe("TH% ");
  });
});

function candidate(
  value: string,
  kind: NativeCompletionCandidate["kind"],
): NativeCompletionCandidate {
  return { value, display: value, kind };
}

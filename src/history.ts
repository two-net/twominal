import { invoke } from "@tauri-apps/api/core";

export class History {
  entries: string[] = [];
  private index = -1;
  private draft = "";

  async load(): Promise<void> {
    this.entries = await invoke<string[]>("history_load");
  }

  async persist(): Promise<void> {
    await invoke("history_save", { entries: this.entries });
  }

  add(line: string): void {
    const t = line.trim();
    if (!t) return;
    if (this.entries[this.entries.length - 1] === t) {
      this.index = -1;
      this.draft = "";
      return;
    }
    this.entries.push(t);
    if (this.entries.length > 2000) this.entries.shift();
    this.index = -1;
    this.draft = "";
    void this.persist();
  }

  suggest(prefix: string): string | null {
    if (!prefix) return null;
    for (let i = this.entries.length - 1; i >= 0; i--) {
      const e = this.entries[i];
      if (e !== prefix && e.startsWith(prefix)) return e;
    }
    return null;
  }

  startWalk(draft: string): void {
    this.draft = draft;
    this.index = this.entries.length;
  }

  prev(draft: string): string | null {
    if (this.index < 0) this.startWalk(draft);
    if (this.index <= 0) return this.entries[0] ?? null;
    this.index -= 1;
    return this.entries[this.index] ?? null;
  }

  next(): string | null {
    if (this.index < 0) return null;
    this.index += 1;
    if (this.index >= this.entries.length) {
      this.index = -1;
      return this.draft;
    }
    return this.entries[this.index] ?? null;
  }
}

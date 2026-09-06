export class CompleteMenu {
  readonly el: HTMLElement;
  items: string[] = [];
  index = 0;
  onPick: (item: string) => void = () => {};

  constructor() {
    this.el = document.createElement("div");
    this.el.className = "complete-box";
    this.el.hidden = true;
    this.el.addEventListener("mousedown", (e) => {
      e.preventDefault();
      const row = (e.target as HTMLElement).closest("[data-i]");
      if (!row) return;
      const i = Number((row as HTMLElement).dataset.i);
      if (!Number.isNaN(i)) this.pick(i);
    });
  }

  get open(): boolean {
    return !this.el.hidden && this.items.length > 0;
  }

  show(items: string[]): void {
    this.items = items;
    this.index = 0;
    this.paint();
    this.el.hidden = items.length === 0;
  }

  hide(): void {
    this.items = [];
    this.index = 0;
    this.el.hidden = true;
    this.el.replaceChildren();
  }

  move(delta: number): void {
    if (!this.items.length) return;
    this.index = (this.index + delta + this.items.length) % this.items.length;
    this.paint();
  }

  current(): string | null {
    return this.items[this.index] ?? null;
  }

  pick(i = this.index): void {
    const item = this.items[i];
    if (!item) return;
    this.hide();
    this.onPick(item);
  }

  private paint(): void {
    this.el.replaceChildren();
    this.items.forEach((item, i) => {
      const row = document.createElement("div");
      row.dataset.i = String(i);
      row.textContent = item;
      if (i === this.index) row.classList.add("active");
      this.el.append(row);
    });
    const active = this.el.querySelector(".active");
    active?.scrollIntoView({ block: "nearest" });
  }
}

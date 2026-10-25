// Tabs (UI-02): the window's tab list and its tab bar. The window's frontend owns its tabs;
// each tab holds the id of its session.

export class TabModel {
  constructor() {
    this.tabs = [];
    this.activeId = null;
    this.nextId = 1;
  }

  get count() {
    return this.tabs.length;
  }

  get active() {
    return this.get(this.activeId);
  }

  get(id) {
    return this.tabs.find((tab) => tab.id === id) ?? null;
  }

  indexOf(id) {
    return this.tabs.findIndex((tab) => tab.id === id);
  }

  /** Adds a tab at the end, like other Mac apps, and makes it active. */
  add(data = {}) {
    const tab = { ...data, id: this.nextId++ };
    this.tabs.push(tab);
    this.activeId = tab.id;
    return tab;
  }

  /** Removes a tab. If it was active, its right neighbour becomes active, or the left one. */
  close(id) {
    const index = this.indexOf(id);
    if (index < 0) return null;
    const [tab] = this.tabs.splice(index, 1);
    if (this.activeId === id) this.activeId = (this.tabs[index] ?? this.tabs[index - 1])?.id ?? null;
    return tab;
  }

  activate(id) {
    if (!this.get(id)) return false;
    this.activeId = id;
    return true;
  }

  /** Activates the tab at a 0-based position, if there is one. */
  activateIndex(index) {
    const tab = this.tabs[index];
    return tab ? this.activate(tab.id) : false;
  }

  /** Activates the next (+1) or previous (-1) tab, wrapping around. */
  cycle(delta) {
    if (this.count === 0) return false;
    const index = (this.indexOf(this.activeId) + delta + this.count) % this.count;
    return this.activateIndex(index);
  }

  /** Moves a tab to `index`, clamped to the ends. Returns whether anything moved. */
  move(id, index) {
    const from = this.indexOf(id);
    if (from < 0) return false;
    const to = Math.max(0, Math.min(this.count - 1, index));
    if (to === from) return false;
    const [tab] = this.tabs.splice(from, 1);
    this.tabs.splice(to, 0, tab);
    return true;
  }
}

/** The tab bar is hidden while the window has a single tab, unless the setting says otherwise. */
export function tabBarVisible(count, alwaysShow) {
  return Boolean(alwaysShow) || count > 1;
}

const reducedMotion = () => window.matchMedia("(prefers-reduced-motion: reduce)").matches;

export class TabBar {
  /**
   * @param {HTMLElement} nav
   * @param {TabModel} model
   * @param {{ onSelect(id: number): void, onClose(id: number): void, onNew(): void, onMove(id: number, index: number): void }} handlers
   */
  constructor(nav, model, handlers) {
    this.nav = nav;
    this.list = nav.querySelector(".tabs");
    this.model = model;
    this.handlers = handlers;
    this.elements = new Map();
    nav.querySelector(".new-tab").addEventListener("click", () => handlers.onNew());
    this.list.addEventListener("pointerdown", (event) => this.startDrag(event));
  }

  setVisible(visible) {
    this.nav.hidden = !visible;
  }

  /** Brings the bar up to date. With `flip`, tabs that changed place slide there. */
  render({ flip = false } = {}) {
    const before = flip ? this.positions() : new Map();
    const seen = new Set();
    for (const tab of this.model.tabs) {
      let element = this.elements.get(tab.id);
      if (!element) {
        element = this.createTab(tab.id);
        this.elements.set(tab.id, element);
        if (!this.nav.hidden) element.classList.add("entering");
      }
      element.setAttribute("aria-selected", String(tab.id === this.model.activeId));
      element.querySelector(".title").textContent = tab.title;
      element.title = tab.title;
      element.style.transform = "";
      this.list.append(element);
      seen.add(tab.id);
    }
    for (const [id, element] of this.elements) {
      if (!seen.has(id)) {
        element.remove();
        this.elements.delete(id);
      }
    }
    if (flip && !reducedMotion()) this.slideFrom(before);
  }

  positions() {
    return new Map([...this.elements].map(([id, element]) => [id, element.getBoundingClientRect().left]));
  }

  slideFrom(before) {
    for (const [id, left] of before) {
      const element = this.elements.get(id);
      const dx = element ? left - element.getBoundingClientRect().left : 0;
      if (Math.abs(dx) < 1) continue;
      element.animate([{ transform: `translateX(${dx}px)` }, { transform: "none" }], {
        duration: 160,
        easing: "cubic-bezier(0.2, 0.8, 0.2, 1)",
      });
    }
  }

  createTab(id) {
    const element = document.createElement("div");
    element.className = "tab";
    element.setAttribute("role", "tab");
    element.dataset.id = String(id);
    element.addEventListener("animationend", () => element.classList.remove("entering"));

    const mark = document.createElement("span");
    mark.className = "mark";
    mark.setAttribute("aria-hidden", "true");
    const title = document.createElement("span");
    title.className = "title";
    const close = document.createElement("button");
    close.type = "button";
    close.className = "close";
    close.tabIndex = -1;
    close.setAttribute("aria-label", "Close tab");
    close.textContent = "×";
    close.addEventListener("click", () => this.handlers.onClose(id));

    element.append(mark, title, close);
    return element;
  }

  /** Selects on press, like Safari's tabs, and reorders when dragged. */
  startDrag(event) {
    const element = event.target.closest(".tab");
    if (!element || event.button !== 0 || event.target.closest(".close")) return;
    const id = Number(element.dataset.id);
    this.handlers.onSelect(id);

    const startX = event.clientX;
    let dragging = false;
    element.setPointerCapture(event.pointerId);

    const move = (moveEvent) => {
      const dx = moveEvent.clientX - startX;
      if (!dragging && Math.abs(dx) < 4) return;
      dragging = true;
      element.classList.add("dragging");
      element.style.transform = `translateX(${dx}px)`;
    };
    const end = (endEvent) => {
      element.removeEventListener("pointermove", move);
      element.removeEventListener("pointerup", end);
      element.removeEventListener("pointercancel", end);
      if (!dragging) return;
      element.classList.remove("dragging");
      this.handlers.onMove(id, this.dropIndex(element, endEvent.clientX));
      this.render({ flip: true });
    };
    element.addEventListener("pointermove", move);
    element.addEventListener("pointerup", end);
    element.addEventListener("pointercancel", end);
  }

  /** The index a dragged tab lands at: the number of other tabs whose centre it has passed. */
  dropIndex(dragged, x) {
    let index = 0;
    for (const other of this.list.children) {
      if (other === dragged) continue;
      const rect = other.getBoundingClientRect();
      if (x > rect.left + rect.width / 2) index++;
    }
    return index;
  }
}

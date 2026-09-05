import {
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { MAX_TABS, type TerminalTab } from "./tabState";
import {
  serializeTabDragPayload,
  TAB_DRAG_TYPE,
  TabStrip,
  type TabDragPayload,
  type TabStripProps,
} from "./TabStrip";

afterEach(cleanup);

describe("TabStrip", () => {
  it("exposes tabs and their separate close actions accessibly", () => {
    renderTabStrip();

    const firstTab = screen.getByRole("tab", { name: "First shell" });
    const closeButton = screen.getByRole("button", {
      name: "Close First shell",
    });

    expect(firstTab).toHaveAttribute("id", "tab-first");
    expect(firstTab).toHaveAttribute("aria-controls", "panel-first");
    expect(firstTab).toHaveAttribute("aria-selected", "true");
    expect(closeButton.closest("[role='tab']")).toBeNull();
  });

  it("activates adjacent tabs with wrapping arrow navigation", () => {
    const onActivate = vi.fn();
    renderTabStrip({ onActivate });

    fireEvent.keyDown(screen.getByRole("tab", { name: "First shell" }), {
      key: "ArrowLeft",
    });

    expect(onActivate).toHaveBeenCalledWith("second");
    expect(screen.getByRole("tab", { name: "Second shell" })).toHaveFocus();
  });

  it("reorders a focused tab with Alt+Shift+Arrow", () => {
    const onActivate = vi.fn();
    const onMove = vi.fn();
    renderTabStrip({ onActivate, onMove });

    fireEvent.keyDown(screen.getByRole("tab", { name: "Second shell" }), {
      key: "ArrowLeft",
      altKey: true,
      shiftKey: true,
    });

    expect(onMove).toHaveBeenCalledWith("second", 0);
    expect(onActivate).not.toHaveBeenCalled();
  });

  it("reorders tabs with HTML drag and drop", () => {
    const onMove = vi.fn();
    const onDetach = vi.fn();
    renderTabStrip({ onMove, onDetach });
    const dataTransfer = createDataTransfer();
    const first = screen.getByRole("tab", { name: "First shell" });
    const secondContainer = screen
      .getByRole("tab", { name: "Second shell" })
      .closest(".tab-item");

    expect(secondContainer).not.toBeNull();
    fireEvent.dragStart(first, { dataTransfer });
    expect(dataTransfer.types).toEqual([TAB_DRAG_TYPE]);
    expect(dataTransfer.getData("text/plain")).toBe("");
    fireEvent.dragOver(secondContainer as Element, { dataTransfer });
    fireEvent.drop(secondContainer as Element, { dataTransfer });
    dataTransfer.dropEffect = "move";
    fireEvent.dragEnd(first, { dataTransfer, screenX: 900, screenY: 400 });

    expect(onMove).toHaveBeenCalledWith("first", 1);
    expect(onDetach).not.toHaveBeenCalled();
  });

  it("requests a cross-window transfer when an external tab is dropped", () => {
    const onExternalDrop = vi.fn();
    renderTabStrip({ onExternalDrop });
    const payload = externalDragPayload();
    const dataTransfer = createDataTransfer({
      [TAB_DRAG_TYPE]: serializeTabDragPayload(payload),
    });
    const secondContainer = screen
      .getByRole("tab", { name: "Second shell" })
      .closest(".tab-item");

    fireEvent.dragOver(secondContainer as Element, { dataTransfer });
    fireEvent.drop(secondContainer as Element, { dataTransfer });

    expect(onExternalDrop).toHaveBeenCalledWith(payload, 1);
  });

  it("accepts an external tab at the end of the tab strip", () => {
    const onExternalDrop = vi.fn();
    renderTabStrip({ onExternalDrop });
    const payload = externalDragPayload();
    const dataTransfer = createDataTransfer({
      [TAB_DRAG_TYPE]: serializeTabDragPayload(payload),
    });
    const tabList = screen.getByRole("tablist", { name: "Terminal tabs" });

    fireEvent.dragOver(tabList, { dataTransfer });
    expect(tabList).toHaveClass("is-drop-target");
    fireEvent.drop(tabList, { dataTransfer });

    expect(onExternalDrop).toHaveBeenCalledWith(payload, 2);
  });

  it("requests detach when an unclaimed drag ends", () => {
    const onDetach = vi.fn();
    const onDragStarted = vi.fn();
    renderTabStrip({ onDetach, onDragStarted });
    const dataTransfer = createDataTransfer();
    const first = screen.getByRole("tab", { name: "First shell" });

    fireEvent.dragStart(first, { dataTransfer });
    fireEvent.dragEnd(first, { dataTransfer });

    expect(onDragStarted).toHaveBeenCalledOnce();
    expect(onDetach).toHaveBeenCalledWith(onDragStarted.mock.calls[0]?.[0]);
  });

  it("does not detach an Escape-cancelled drag", () => {
    const onDetach = vi.fn();
    renderTabStrip({ onDetach });
    const dataTransfer = createDataTransfer();
    const first = screen.getByRole("tab", { name: "First shell" });

    fireEvent.dragStart(first, { dataTransfer });
    fireEvent.keyDown(window, { key: "Escape" });
    fireEvent.dragEnd(first, { dataTransfer });

    expect(onDetach).not.toHaveBeenCalled();
  });

  it("ignores malformed external drag payloads", () => {
    const onExternalDrop = vi.fn();
    renderTabStrip({ onExternalDrop });
    const dataTransfer = createDataTransfer({
      [TAB_DRAG_TYPE]: "not valid JSON",
    });
    const tabList = screen.getByRole("tablist", { name: "Terminal tabs" });

    fireEvent.dragOver(tabList, { dataTransfer });
    fireEvent.drop(tabList, { dataTransfer });

    expect(onExternalDrop).not.toHaveBeenCalled();
  });

  it("disables new tabs at the workspace cap", () => {
    const tabs = Array.from({ length: MAX_TABS }, (_, index) =>
      createTab(`tab-${index}`, `Terminal ${index + 1}`),
    );

    renderTabStrip({ tabs, activeTabId: tabs[0]?.id ?? null });

    expect(
      screen.getByRole("button", { name: "New terminal tab" }),
    ).toBeDisabled();
  });

  it("makes a closing tab inert during its exit transition", () => {
    const onActivate = vi.fn();
    const onClose = vi.fn();
    renderTabStrip({ closingTabIds: ["first"], onActivate, onClose });

    const tab = screen.getByRole("tab", { name: "First shell" });
    expect(tab).toHaveAttribute("aria-disabled", "true");
    expect(tab).toHaveAttribute("draggable", "false");
    expect(
      screen.getByRole("button", { name: "Close First shell" }),
    ).toBeDisabled();
    fireEvent.click(tab);
    expect(onActivate).not.toHaveBeenCalled();
  });
});

function renderTabStrip(overrides: Partial<TabStripProps> = {}) {
  const tabs = [
    createTab("first", "First shell"),
    createTab("second", "Second shell"),
  ];

  return render(
    <TabStrip
      tabs={tabs}
      activeTabId="first"
      onActivate={vi.fn()}
      onNew={vi.fn()}
      onClose={vi.fn()}
      onMove={vi.fn()}
      windowLabel="main"
      {...overrides}
    />,
  );
}

function externalDragPayload(): TabDragPayload {
  return {
    version: 1,
    dragId: "external-drag",
    sourceWindowLabel: "twominal-child",
    tabId: "remote-tab",
  };
}

function createDataTransfer(initial: Record<string, string> = {}) {
  const data = new Map(Object.entries(initial));
  return {
    dropEffect: "none",
    effectAllowed: "uninitialized",
    get types() {
      return [...data.keys()];
    },
    getData: vi.fn((type: string) => data.get(type) ?? ""),
    setData: vi.fn((type: string, value: string) => {
      data.set(type, value);
    }),
    clearData: vi.fn((type?: string) => {
      if (type) data.delete(type);
      else data.clear();
    }),
  };
}

function createTab(id: string, title: string): TerminalTab {
  return {
    id,
    title,
    terminalState: { type: "starting" },
    restartKey: 0,
  };
}

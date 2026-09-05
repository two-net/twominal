import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { AppErrorBoundary } from "./AppErrorBoundary";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

function BrokenContent(): never {
  throw new Error("private command and environment details");
}

describe("AppErrorBoundary", () => {
  it("renders its children while the application is healthy", () => {
    render(
      <AppErrorBoundary>
        <p>Terminal workspace</p>
      </AppErrorBoundary>,
    );

    expect(screen.getByText("Terminal workspace")).toBeVisible();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("shows an accessible generic recovery screen without exposing errors", () => {
    vi.spyOn(console, "error").mockImplementation(() => undefined);

    render(
      <AppErrorBoundary>
        <BrokenContent />
      </AppErrorBoundary>,
    );

    const alert = screen.getByRole("alert");
    expect(alert).toHaveAccessibleName("Twominal needs to reload");
    expect(alert).toHaveAccessibleDescription(
      "This window encountered an unexpected problem. Reload it to start a fresh terminal workspace.",
    );
    expect(
      screen.queryByText(/private command and environment details/i),
    ).not.toBeInTheDocument();
  });

  it("uses the injected reload action", () => {
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    const onReload = vi.fn();

    render(
      <AppErrorBoundary onReload={onReload}>
        <BrokenContent />
      </AppErrorBoundary>,
    );

    fireEvent.click(screen.getByRole("button", { name: "Reload Twominal" }));
    expect(onReload).toHaveBeenCalledOnce();
  });
});

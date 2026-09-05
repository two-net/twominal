import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { SplashScreen } from "./SplashScreen";

describe("SplashScreen", () => {
  it("announces startup without adding a timer", () => {
    render(<SplashScreen visible />);
    expect(
      screen.getByRole("status", { name: "Twominal is starting" }),
    ).toHaveTextContent("Twominalterminal, twice as friendly");
  });

  it("becomes inert as soon as startup completes", () => {
    const { container } = render(<SplashScreen visible={false} />);
    const splash = container.querySelector(".splash-screen");
    expect(splash).toHaveAttribute("aria-hidden", "true");
    expect(splash).toHaveClass("is-hidden");
  });
});

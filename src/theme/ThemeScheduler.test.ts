import { describe, expect, it, vi } from "vitest";
import type { AppearanceConfig } from "../config/types";
import {
  ThemeScheduler,
  calculateSolarTimes,
  calculateSolarUtcHours,
  isValidCoordinates,
  nextThemeReevaluationDelay,
  resolveThemeAppearance,
} from "./ThemeScheduler";

const appearance = (
  mode: AppearanceConfig["mode"],
  latitude: number | null = null,
  longitude: number | null = null,
): AppearanceConfig => ({ mode, latitude, longitude });

const localDate = (hour: number, minute = 0): Date =>
  new Date(2026, 8, 3, hour, minute, 0, 0);

describe("resolveThemeAppearance", () => {
  it("resolves explicit and system modes", () => {
    expect(resolveThemeAppearance(appearance("light"), true)).toBe("light");
    expect(resolveThemeAppearance(appearance("dark"), false)).toBe("dark");
    expect(resolveThemeAppearance(appearance("system"), true)).toBe("dark");
    expect(resolveThemeAppearance(appearance("system"), false)).toBe("light");
  });

  it("uses inclusive 06:00 and exclusive 18:00 fallback boundaries", () => {
    const scheduled = appearance("sunSchedule");
    expect(resolveThemeAppearance(scheduled, false, localDate(5, 59))).toBe(
      "dark",
    );
    expect(resolveThemeAppearance(scheduled, true, localDate(6))).toBe(
      "light",
    );
    expect(resolveThemeAppearance(scheduled, true, localDate(17, 59))).toBe(
      "light",
    );
    expect(resolveThemeAppearance(scheduled, false, localDate(18))).toBe(
      "dark",
    );
  });

  it("uses the fallback for invalid and polar coordinates", () => {
    expect(
      resolveThemeAppearance(
        appearance("sunSchedule", 91, 0),
        true,
        localDate(12),
      ),
    ).toBe("light");
    expect(calculateSolarTimes(localDate(12), 90, 0)).toBeNull();
    expect(
      resolveThemeAppearance(
        appearance("sunSchedule", 90, 0),
        false,
        localDate(20),
      ),
    ).toBe("dark");
  });
});

describe("solar calculations", () => {
  it("calculates plausible UTC sunrise and sunset at Greenwich solstice", () => {
    const result = calculateSolarUtcHours(
      new Date(2026, 5, 21, 12),
      51.4769,
      0,
    );

    expect(result).not.toBeNull();
    expect(result?.sunrise).toBeGreaterThan(3);
    expect(result?.sunrise).toBeLessThan(5);
    expect(result?.sunset).toBeGreaterThan(19.5);
    expect(result?.sunset).toBeLessThan(21.5);
  });

  it("uses calculated transitions for coordinates in the local timezone", () => {
    const date = new Date(2026, 2, 20, 12);
    const timezoneLongitude = (-date.getTimezoneOffset() / 60) * 15;
    const solarTimes = calculateSolarTimes(date, 0, timezoneLongitude);

    expect(solarTimes).not.toBeNull();
    if (solarTimes === null) {
      return;
    }
    const scheduled = appearance("sunSchedule", 0, timezoneLongitude);
    const afterSunrise = new Date(solarTimes.sunrise.getTime() + 60_000);
    const afterSunset = new Date(solarTimes.sunset.getTime() + 60_000);
    expect(resolveThemeAppearance(scheduled, true, afterSunrise)).toBe("light");
    expect(resolveThemeAppearance(scheduled, false, afterSunset)).toBe("dark");
  });

  it("rejects incomplete, non-finite, and out-of-range coordinates", () => {
    expect(isValidCoordinates(null, null)).toBe(false);
    expect(isValidCoordinates(10, null)).toBe(false);
    expect(isValidCoordinates(Number.NaN, 10)).toBe(false);
    expect(isValidCoordinates(10, Number.POSITIVE_INFINITY)).toBe(false);
    expect(isValidCoordinates(-90.1, 10)).toBe(false);
    expect(isValidCoordinates(10, 180.1)).toBe(false);
    expect(isValidCoordinates(-90, 180)).toBe(true);
  });
});

describe("nextThemeReevaluationDelay", () => {
  it("does not poll fixed or system themes", () => {
    expect(nextThemeReevaluationDelay(appearance("light"), localDate(12))).toBe(
      null,
    );
    expect(nextThemeReevaluationDelay(appearance("system"), localDate(12))).toBe(
      null,
    );
  });

  it("targets the fallback transition and caps long waits", () => {
    const scheduled = appearance("sunSchedule");
    const nearSunset = nextThemeReevaluationDelay(
      scheduled,
      localDate(17, 30),
    );
    expect(nearSunset).toBe(30 * 60 * 1_000 + 250);

    const longWait = nextThemeReevaluationDelay(scheduled, localDate(7));
    expect(longWait).toBe(6 * 60 * 60 * 1_000);
  });
});

describe("ThemeScheduler", () => {
  it("emits changes, follows system events, and tears down subscriptions", () => {
    let prefersDark = false;
    let systemListener: (() => void) | null = null;
    const unsubscribe = vi.fn();
    const onChange = vi.fn();
    const scheduler = new ThemeScheduler(onChange, {
      now: () => localDate(12),
      systemPrefersDark: () => prefersDark,
      subscribeToSystemAppearance: (listener) => {
        systemListener = listener;
        return unsubscribe;
      },
      setTimer: vi.fn(() => 1 as unknown as ReturnType<typeof setTimeout>),
      clearTimer: vi.fn(),
    });

    scheduler.start(appearance("system"));
    expect(onChange).toHaveBeenLastCalledWith("light");

    prefersDark = true;
    const notifySystemChange = systemListener as (() => void) | null;
    notifySystemChange?.();
    expect(onChange).toHaveBeenLastCalledWith("dark");
    expect(unsubscribe).toHaveBeenCalledTimes(1);

    scheduler.stop();
    expect(unsubscribe).toHaveBeenCalledTimes(2);
  });
});

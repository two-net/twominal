import type { AppearanceConfig } from "../config/types";

const ASTRONOMICAL_ZENITH_DEGREES = 90.833;
const FALLBACK_SUNRISE_HOUR = 6;
const FALLBACK_SUNSET_HOUR = 18;
const MIN_REEVALUATION_DELAY_MS = 1_000;
const MAX_REEVALUATION_DELAY_MS = 6 * 60 * 60 * 1_000;
const TRANSITION_EPSILON_MS = 250;

export type ResolvedAppearance = "light" | "dark";

export interface SolarUtcHours {
  sunrise: number;
  sunset: number;
}

export interface SolarTimes {
  sunrise: Date;
  sunset: Date;
}

export interface ThemeSchedulerEnvironment {
  now(): Date;
  systemPrefersDark(): boolean;
  subscribeToSystemAppearance(listener: () => void): () => void;
  setTimer(listener: () => void, delayMs: number): ReturnType<typeof setTimeout>;
  clearTimer(timer: ReturnType<typeof setTimeout>): void;
}

export function isValidCoordinates(
  latitude: number | null,
  longitude: number | null,
): boolean {
  return (
    latitude !== null &&
    longitude !== null &&
    Number.isFinite(latitude) &&
    Number.isFinite(longitude) &&
    latitude >= -90 &&
    latitude <= 90 &&
    longitude >= -180 &&
    longitude <= 180
  );
}

export function calculateSolarUtcHours(
  date: Date,
  latitude: number,
  longitude: number,
): SolarUtcHours | null {
  if (
    !isValidCoordinates(latitude, longitude) ||
    Number.isNaN(date.getTime())
  ) {
    return null;
  }

  const sunrise = calculateSolarEventUtcHour(
    date,
    latitude,
    longitude,
    true,
  );
  const sunset = calculateSolarEventUtcHour(
    date,
    latitude,
    longitude,
    false,
  );

  return sunrise === null || sunset === null ? null : { sunrise, sunset };
}

export function calculateSolarTimes(
  date: Date,
  latitude: number,
  longitude: number,
): SolarTimes | null {
  const utcHours = calculateSolarUtcHours(date, latitude, longitude);
  if (utcHours === null) {
    return null;
  }

  return {
    sunrise: utcHourOnLocalDate(date, utcHours.sunrise),
    sunset: utcHourOnLocalDate(date, utcHours.sunset),
  };
}

export function resolveThemeAppearance(
  appearance: AppearanceConfig,
  systemPrefersDark: boolean,
  now: Date = new Date(),
): ResolvedAppearance {
  switch (appearance.mode) {
    case "light":
      return "light";
    case "dark":
      return "dark";
    case "system":
      return systemPrefersDark ? "dark" : "light";
    case "sunSchedule": {
      const solarTimes = getSolarTimes(appearance, now);
      if (solarTimes === null) {
        return resolveFallbackSchedule(now);
      }
      return isWithinDaylight(now, solarTimes) ? "light" : "dark";
    }
  }
}

export function nextThemeReevaluationDelay(
  appearance: AppearanceConfig,
  now: Date = new Date(),
): number | null {
  if (appearance.mode !== "sunSchedule") {
    return null;
  }

  if (Number.isNaN(now.getTime())) {
    return MAX_REEVALUATION_DELAY_MS;
  }

  const nextTransition = findNextSolarTransition(appearance, now);
  const fallbackTransition = nextFallbackTransition(now);
  const transition = nextTransition ?? fallbackTransition;
  const untilTransition =
    transition.getTime() - now.getTime() + TRANSITION_EPSILON_MS;

  return Math.max(
    MIN_REEVALUATION_DELAY_MS,
    Math.min(untilTransition, MAX_REEVALUATION_DELAY_MS),
  );
}

export class ThemeScheduler {
  private appearance: AppearanceConfig | null = null;
  private active = false;
  private lastResolved: ResolvedAppearance | null = null;
  private timer: ReturnType<typeof setTimeout> | null = null;
  private unsubscribeSystemAppearance: (() => void) | null = null;

  constructor(
    private readonly onAppearanceChange: (
      appearance: ResolvedAppearance,
    ) => void,
    private readonly environment: ThemeSchedulerEnvironment =
      createBrowserThemeEnvironment(),
  ) {}

  start(appearance: AppearanceConfig): void {
    this.appearance = appearance;
    this.active = true;
    this.lastResolved = null;
    this.refresh();
  }

  update(appearance: AppearanceConfig): void {
    this.appearance = appearance;
    if (this.active) {
      this.refresh();
    }
  }

  stop(): void {
    this.active = false;
    this.clearScheduledWork();
    this.lastResolved = null;
  }

  private refresh(): void {
    this.clearScheduledWork();
    if (!this.active || this.appearance === null) {
      return;
    }

    const resolved = resolveThemeAppearance(
      this.appearance,
      this.environment.systemPrefersDark(),
      this.environment.now(),
    );
    if (resolved !== this.lastResolved) {
      this.lastResolved = resolved;
      this.onAppearanceChange(resolved);
    }

    if (this.appearance.mode === "system") {
      this.unsubscribeSystemAppearance =
        this.environment.subscribeToSystemAppearance(() => {
          this.refresh();
        });
      return;
    }

    const delay = nextThemeReevaluationDelay(
      this.appearance,
      this.environment.now(),
    );
    if (delay !== null) {
      this.timer = this.environment.setTimer(() => {
        this.timer = null;
        this.refresh();
      }, delay);
    }
  }

  private clearScheduledWork(): void {
    if (this.timer !== null) {
      this.environment.clearTimer(this.timer);
      this.timer = null;
    }
    this.unsubscribeSystemAppearance?.();
    this.unsubscribeSystemAppearance = null;
  }
}

export function createBrowserThemeEnvironment(): ThemeSchedulerEnvironment {
  return {
    now: () => new Date(),
    systemPrefersDark: () => getSystemAppearanceQuery()?.matches ?? false,
    subscribeToSystemAppearance: (listener) => {
      const query = getSystemAppearanceQuery();
      if (query === null) {
        return () => undefined;
      }
      query.addEventListener("change", listener);
      return () => {
        query.removeEventListener("change", listener);
      };
    },
    setTimer: (listener, delayMs) => globalThis.setTimeout(listener, delayMs),
    clearTimer: (timer) => {
      globalThis.clearTimeout(timer);
    },
  };
}

function getSystemAppearanceQuery(): MediaQueryList | null {
  return typeof window === "undefined" || typeof window.matchMedia !== "function"
    ? null
    : window.matchMedia("(prefers-color-scheme: dark)");
}

function getSolarTimes(
  appearance: AppearanceConfig,
  date: Date,
): SolarTimes | null {
  const { latitude, longitude } = appearance;
  if (
    !isValidCoordinates(latitude, longitude) ||
    latitude === null ||
    longitude === null
  ) {
    return null;
  }
  return calculateSolarTimes(date, latitude, longitude);
}

function findNextSolarTransition(
  appearance: AppearanceConfig,
  now: Date,
): Date | null {
  const { latitude, longitude } = appearance;
  if (
    !isValidCoordinates(latitude, longitude) ||
    latitude === null ||
    longitude === null
  ) {
    return null;
  }

  for (let dayOffset = 0; dayOffset <= 1; dayOffset += 1) {
    const date = new Date(
      now.getFullYear(),
      now.getMonth(),
      now.getDate() + dayOffset,
      12,
    );
    const solarTimes = calculateSolarTimes(
      date,
      latitude,
      longitude,
    );
    if (solarTimes === null) {
      return null;
    }

    const futureTransitions = [solarTimes.sunrise, solarTimes.sunset]
      .filter((transition) => transition.getTime() > now.getTime())
      .sort((left, right) => left.getTime() - right.getTime());
    if (futureTransitions[0] !== undefined) {
      return futureTransitions[0];
    }
  }

  return null;
}

function resolveFallbackSchedule(now: Date): ResolvedAppearance {
  if (Number.isNaN(now.getTime())) {
    return "dark";
  }
  const minutes = now.getHours() * 60 + now.getMinutes();
  return minutes >= FALLBACK_SUNRISE_HOUR * 60 &&
    minutes < FALLBACK_SUNSET_HOUR * 60
    ? "light"
    : "dark";
}

function nextFallbackTransition(now: Date): Date {
  const sunrise = new Date(
    now.getFullYear(),
    now.getMonth(),
    now.getDate(),
    FALLBACK_SUNRISE_HOUR,
  );
  const sunset = new Date(
    now.getFullYear(),
    now.getMonth(),
    now.getDate(),
    FALLBACK_SUNSET_HOUR,
  );

  if (now.getTime() < sunrise.getTime()) {
    return sunrise;
  }
  if (now.getTime() < sunset.getTime()) {
    return sunset;
  }
  return new Date(
    now.getFullYear(),
    now.getMonth(),
    now.getDate() + 1,
    FALLBACK_SUNRISE_HOUR,
  );
}

function isWithinDaylight(now: Date, solarTimes: SolarTimes): boolean {
  const time = now.getTime();
  const sunrise = solarTimes.sunrise.getTime();
  const sunset = solarTimes.sunset.getTime();

  if (sunrise <= sunset) {
    return time >= sunrise && time < sunset;
  }
  return time >= sunrise || time < sunset;
}

function calculateSolarEventUtcHour(
  date: Date,
  latitude: number,
  longitude: number,
  sunrise: boolean,
): number | null {
  const dayOfYear = getDayOfYear(date);
  const longitudeHour = longitude / 15;
  const targetHour = sunrise ? 6 : 18;
  const approximateTime =
    dayOfYear + (targetHour - longitudeHour) / 24;
  const meanAnomaly = 0.9856 * approximateTime - 3.289;
  const trueLongitude = normalizeDegrees(
    meanAnomaly +
      1.916 * sinDegrees(meanAnomaly) +
      0.02 * sinDegrees(2 * meanAnomaly) +
      282.634,
  );

  let rightAscension = normalizeDegrees(
    toDegrees(Math.atan(0.91764 * Math.tan(toRadians(trueLongitude)))),
  );
  const longitudeQuadrant = Math.floor(trueLongitude / 90) * 90;
  const rightAscensionQuadrant = Math.floor(rightAscension / 90) * 90;
  rightAscension =
    (rightAscension + longitudeQuadrant - rightAscensionQuadrant) / 15;

  const sinDeclination = 0.39782 * sinDegrees(trueLongitude);
  const cosDeclination = Math.cos(Math.asin(sinDeclination));
  const cosineHourAngle =
    (cosDegrees(ASTRONOMICAL_ZENITH_DEGREES) -
      sinDeclination * sinDegrees(latitude)) /
    (cosDeclination * cosDegrees(latitude));

  if (cosineHourAngle > 1 || cosineHourAngle < -1) {
    return null;
  }

  const hourAngleDegrees = sunrise
    ? 360 - toDegrees(Math.acos(cosineHourAngle))
    : toDegrees(Math.acos(cosineHourAngle));
  const hourAngle = hourAngleDegrees / 15;
  const localMeanTime =
    hourAngle +
    rightAscension -
    0.06571 * approximateTime -
    6.622;
  return normalizeHours(localMeanTime - longitudeHour);
}

function getDayOfYear(date: Date): number {
  const current = Date.UTC(date.getFullYear(), date.getMonth(), date.getDate());
  const start = Date.UTC(date.getFullYear(), 0, 0);
  return Math.floor((current - start) / 86_400_000);
}

function utcHourOnLocalDate(localDate: Date, utcHour: number): Date {
  const year = localDate.getFullYear();
  const month = localDate.getMonth();
  const day = localDate.getDate();
  const base = Date.UTC(year, month, day);
  const localNoon = new Date(year, month, day, 12).getTime();

  const matchingCandidates = [-1, 0, 1]
    .map((dayOffset) =>
      new Date(base + dayOffset * 86_400_000 + utcHour * 3_600_000),
    )
    .filter(
      (candidate) =>
        candidate.getFullYear() === year &&
        candidate.getMonth() === month &&
        candidate.getDate() === day,
    )
    .sort(
      (left, right) =>
        Math.abs(left.getTime() - localNoon) -
        Math.abs(right.getTime() - localNoon),
    );

  if (matchingCandidates[0] !== undefined) {
    return matchingCandidates[0];
  }

  const offsetMinutes = new Date(year, month, day, 12).getTimezoneOffset();
  return new Date(
    base + utcHour * 3_600_000 + offsetMinutes * 60_000,
  );
}

function normalizeDegrees(value: number): number {
  return ((value % 360) + 360) % 360;
}

function normalizeHours(value: number): number {
  return ((value % 24) + 24) % 24;
}

function toRadians(degrees: number): number {
  return (degrees * Math.PI) / 180;
}

function toDegrees(radians: number): number {
  return (radians * 180) / Math.PI;
}

function sinDegrees(degrees: number): number {
  return Math.sin(toRadians(degrees));
}

function cosDegrees(degrees: number): number {
  return Math.cos(toRadians(degrees));
}

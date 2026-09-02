import { invoke } from "@tauri-apps/api/core";

export type ThemeMode = "dark" | "light" | "auto";

export interface SolarInfo {
  is_day: boolean;
  current_time: string;
  sunrise_time: string;
  sunset_time: string;
  recommended_theme: "light" | "dark";
}

export type ThemeChangeListener = (mode: ThemeMode) => void;

export class ThemeManager {
  private static instance: ThemeManager;
  private currentMode: ThemeMode = "auto";
  private listeners: Set<ThemeChangeListener> = new Set();
  private timer: number | null = null;

  private constructor() {
    this.loadSettings();
    this.apply();
    this.startAutoCheck();
  }

  public static getInstance(): ThemeManager {
    if (!ThemeManager.instance) {
      ThemeManager.instance = new ThemeManager();
    }
    return ThemeManager.instance;
  }

  private loadSettings(): void {
    try {
      const saved = localStorage.getItem("twominal_theme_mode") as ThemeMode;
      if (saved && ["dark", "light", "auto"].includes(saved)) {
        this.currentMode = saved;
      } else {
        this.currentMode = "auto";
      }
    } catch {
      this.currentMode = "auto";
    }
  }

  public saveSettings(): void {
    try {
      localStorage.setItem("twominal_theme_mode", this.currentMode);
    } catch {
      // Ignore
    }
  }

  public getMode(): ThemeMode {
    return this.currentMode;
  }

  public setMode(mode: ThemeMode): void {
    this.currentMode = mode;
    this.saveSettings();
    this.apply();
    this.notify();
  }

  public onThemeChange(listener: ThemeChangeListener): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  private notify(): void {
    for (const listener of this.listeners) {
      try {
        listener(this.currentMode);
      } catch (err) {
        console.error("Theme listener error:", err);
      }
    }
  }

  public async apply(): Promise<void> {
    const checkDark = document.getElementById("check-dark");
    const checkLight = document.getElementById("check-light");
    const checkAuto = document.getElementById("check-auto");
    const themeIcon = document.getElementById("theme-icon");
    const sbThemeInfo = document.getElementById("sb-theme-info");
    const themeDropdown = document.getElementById("theme-dropdown");

    checkDark?.classList.toggle("hidden", this.currentMode !== "dark");
    checkLight?.classList.toggle("hidden", this.currentMode !== "light");
    checkAuto?.classList.toggle("hidden", this.currentMode !== "auto");
    themeDropdown?.classList.add("hidden");

    if (this.currentMode === "dark") {
      this.applyDarkTheme();
      if (themeIcon) themeIcon.textContent = "🌙";
      if (sbThemeInfo) sbThemeInfo.textContent = "Always Dark";
    } else if (this.currentMode === "light") {
      this.applyLightTheme();
      if (themeIcon) themeIcon.textContent = "☀️";
      if (sbThemeInfo) sbThemeInfo.textContent = "Always Light";
    } else if (this.currentMode === "auto") {
      await this.checkSunsetSunriseTheme();
    }
  }

  public applyDarkTheme(): void {
    document.documentElement.classList.add("dark");
    const win = document.getElementById("window-container");
    if (win) {
      win.style.backgroundColor = "#0d1117";
    }
  }

  public applyLightTheme(): void {
    document.documentElement.classList.remove("dark");
    const win = document.getElementById("window-container");
    if (win) {
      win.style.backgroundColor = "#ffffff";
    }
  }

  public async checkSunsetSunriseTheme(): Promise<void> {
    if (this.currentMode !== "auto") return;

    let isDaytime = true;

    try {
      const solarInfo = await invoke<SolarInfo>("get_solar_theme_info");
      isDaytime = solarInfo.is_day;
    } catch {
      const now = new Date();
      const currentMinutes = now.getHours() * 60 + now.getMinutes();
      const sunriseMinutes = 6 * 60 + 30; // 06:30
      const sunsetMinutes = 18 * 60 + 30; // 18:30
      isDaytime = currentMinutes >= sunriseMinutes && currentMinutes < sunsetMinutes;
    }

    const themeIcon = document.getElementById("theme-icon");
    const sbThemeInfo = document.getElementById("sb-theme-info");

    if (isDaytime) {
      this.applyLightTheme();
      if (themeIcon) themeIcon.textContent = "🌅";
      if (sbThemeInfo) sbThemeInfo.textContent = "Auto: Day (Light)";
    } else {
      this.applyDarkTheme();
      if (themeIcon) themeIcon.textContent = "🌙";
      if (sbThemeInfo) sbThemeInfo.textContent = "Auto: Night (Dark)";
    }
  }

  private startAutoCheck(): void {
    if (this.timer) clearInterval(this.timer);
    this.timer = window.setInterval(() => {
      if (this.currentMode === "auto") {
        this.checkSunsetSunriseTheme();
      }
    }, 60000);
  }
}

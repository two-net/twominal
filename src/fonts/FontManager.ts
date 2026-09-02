export interface FontSettings {
  fontFamily: string;
  customFamily?: string;
  fontSize: number;
  fontWeight: number;
  lineHeight: number;
  letterSpacing: number;
  ligatures: boolean;
}

export interface FontDefinition {
  id: string;
  name: string;
  family: string;
  supportsLigatures: boolean;
  category: "ligature" | "standard" | "system" | "custom";
  description: string;
}

export const AVAILABLE_FONTS: FontDefinition[] = [
  {
    id: "Fira Code",
    name: "Fira Code",
    family: "Fira Code",
    supportsLigatures: true,
    category: "ligature",
    description: "Popular coding font with full programming ligatures (===, !==, =>, <=, !=)",
  },
  {
    id: "JetBrains Mono",
    name: "JetBrains Mono",
    family: "JetBrains Mono",
    supportsLigatures: true,
    category: "ligature",
    description: "Developer font by JetBrains with rich ligature support",
  },
  {
    id: "Cascadia Code",
    name: "Cascadia Code",
    family: "Cascadia Code",
    supportsLigatures: true,
    category: "ligature",
    description: "Microsoft developer font with full coding ligatures",
  },
  {
    id: "Victor Mono",
    name: "Victor Mono",
    family: "Victor Mono",
    supportsLigatures: true,
    category: "ligature",
    description: "Monospace font with cursive italics and coding ligatures",
  },
  {
    id: "Source Code Pro",
    name: "Source Code Pro",
    family: "Source Code Pro",
    supportsLigatures: false,
    category: "standard",
    description: "Adobe's clean mono (does not have coding ligatures)",
  },
  {
    id: "Roboto Mono",
    name: "Roboto Mono",
    family: "Roboto Mono",
    supportsLigatures: false,
    category: "standard",
    description: "Google's geometric mono (does not have coding ligatures)",
  },
  {
    id: "Inconsolata",
    name: "Inconsolata",
    family: "Inconsolata",
    supportsLigatures: false,
    category: "standard",
    description: "Clear fixed-width mono (does not have coding ligatures)",
  },
  {
    id: "Space Mono",
    name: "Space Mono",
    family: "Space Mono",
    supportsLigatures: false,
    category: "standard",
    description: "Retro fixed-width mono (does not have coding ligatures)",
  },
  {
    id: "Menlo, Monaco, Consolas, monospace",
    name: "System Monospace (Menlo/Consolas)",
    family: "Menlo, Monaco, Consolas, monospace",
    supportsLigatures: false,
    category: "system",
    description: "Built-in OS monospace font (does not have coding ligatures)",
  },
];

export const DEFAULT_FONT_SETTINGS: FontSettings = {
  fontFamily: "Fira Code",
  customFamily: "",
  fontSize: 14,
  fontWeight: 400,
  lineHeight: 1.5,
  letterSpacing: 0,
  ligatures: true,
};

export type FontChangeListener = (settings: FontSettings) => void;

export class FontManager {
  private static instance: FontManager;
  private settings: FontSettings = { ...DEFAULT_FONT_SETTINGS };
  private listeners: Set<FontChangeListener> = new Set();

  private constructor() {
    this.loadSettings();
    this.apply();
  }

  public static getInstance(): FontManager {
    if (!FontManager.instance) {
      FontManager.instance = new FontManager();
    }
    return FontManager.instance;
  }

  private loadSettings(): void {
    try {
      const raw = localStorage.getItem("twominal_font_settings");
      if (raw) {
        const parsed = JSON.parse(raw);
        this.settings = {
          fontFamily:
            typeof parsed.fontFamily === "string" && parsed.fontFamily
              ? parsed.fontFamily
              : DEFAULT_FONT_SETTINGS.fontFamily,
          customFamily:
            typeof parsed.customFamily === "string"
              ? parsed.customFamily
              : DEFAULT_FONT_SETTINGS.customFamily,
          fontSize:
            typeof parsed.fontSize === "number"
              ? Math.min(Math.max(Math.round(parsed.fontSize), 9), 40)
              : DEFAULT_FONT_SETTINGS.fontSize,
          fontWeight:
            typeof parsed.fontWeight === "number"
              ? Math.min(Math.max(parsed.fontWeight, 100), 900)
              : DEFAULT_FONT_SETTINGS.fontWeight,
          lineHeight:
            typeof parsed.lineHeight === "number"
              ? Math.min(Math.max(parseFloat(parsed.lineHeight.toFixed(2)), 0.8), 3.0)
              : DEFAULT_FONT_SETTINGS.lineHeight,
          letterSpacing:
            typeof parsed.letterSpacing === "number"
              ? Math.min(Math.max(parseFloat(parsed.letterSpacing.toFixed(2)), -2), 6)
              : DEFAULT_FONT_SETTINGS.letterSpacing,
          ligatures:
            typeof parsed.ligatures === "boolean"
              ? parsed.ligatures
              : DEFAULT_FONT_SETTINGS.ligatures,
        };
        return;
      }

      // Legacy fallback: check twominal_ligatures
      const legacyLig = localStorage.getItem("twominal_ligatures");
      if (legacyLig !== null) {
        this.settings.ligatures = legacyLig === "true";
      }
    } catch (err) {
      console.error("Failed to load font settings:", err);
    }
  }

  public saveSettings(): void {
    try {
      localStorage.setItem("twominal_font_settings", JSON.stringify(this.settings));
      localStorage.setItem("twominal_ligatures", String(this.settings.ligatures));
    } catch (err) {
      console.error("Failed to save font settings:", err);
    }
  }

  public getSettings(): FontSettings {
    return { ...this.settings };
  }

  public doesFontSupportLigatures(family: string): boolean {
    const found = AVAILABLE_FONTS.find(
      (f) => f.family.toLowerCase() === family.toLowerCase() || f.id.toLowerCase() === family.toLowerCase()
    );
    if (found) return found.supportsLigatures;
    if (family === "custom") return true; // Assume custom font might support ligatures
    return false;
  }

  public isCurrentFontLigatureCapable(): boolean {
    return this.doesFontSupportLigatures(this.settings.fontFamily);
  }

  public getEffectiveFontFamily(): string {
    if (this.settings.fontFamily === "custom" && this.settings.customFamily?.trim()) {
      return `"${this.settings.customFamily.trim()}", monospace`;
    }
    if (this.settings.fontFamily.includes(",")) {
      return this.settings.fontFamily;
    }
    return `"${this.settings.fontFamily}", monospace`;
  }

  public updateSettings(partial: Partial<FontSettings>): void {
    this.settings = {
      ...this.settings,
      ...partial,
    };
    this.saveSettings();
    this.apply();
    this.notify();
  }

  public setFontFamily(fontFamily: string, customFamily?: string): void {
    this.settings.fontFamily = fontFamily;
    if (customFamily !== undefined) {
      this.settings.customFamily = customFamily;
    }
    this.saveSettings();
    this.apply();
    this.notify();
  }

  public setFontSize(fontSize: number): number {
    const clamped = Math.min(Math.max(Math.round(fontSize), 9), 40);
    this.settings.fontSize = clamped;
    this.saveSettings();
    this.apply();
    this.notify();
    return clamped;
  }

  public increaseFontSize(step = 1): number {
    return this.setFontSize(this.settings.fontSize + step);
  }

  public decreaseFontSize(step = 1): number {
    return this.setFontSize(this.settings.fontSize - step);
  }

  public resetFontSize(): number {
    return this.setFontSize(DEFAULT_FONT_SETTINGS.fontSize);
  }

  public setFontWeight(fontWeight: number): void {
    this.settings.fontWeight = Math.min(Math.max(fontWeight, 100), 900);
    this.saveSettings();
    this.apply();
    this.notify();
  }

  public setLineHeight(lineHeight: number): void {
    this.settings.lineHeight = Math.min(Math.max(parseFloat(lineHeight.toFixed(2)), 0.8), 3.0);
    this.saveSettings();
    this.apply();
    this.notify();
  }

  public setLetterSpacing(letterSpacing: number): void {
    this.settings.letterSpacing = Math.min(
      Math.max(parseFloat(letterSpacing.toFixed(2)), -2),
      6
    );
    this.saveSettings();
    this.apply();
    this.notify();
  }

  public setLigatures(enabled: boolean): void {
    this.settings.ligatures = enabled;
    this.saveSettings();
    this.apply();
    this.notify();
  }

  public toggleLigatures(): boolean {
    this.settings.ligatures = !this.settings.ligatures;
    this.saveSettings();
    this.apply();
    this.notify();
    return this.settings.ligatures;
  }

  public resetDefaults(): void {
    this.settings = { ...DEFAULT_FONT_SETTINGS };
    this.saveSettings();
    this.apply();
    this.notify();
  }

  public onFontChange(listener: FontChangeListener): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  private notify(): void {
    const s = this.getSettings();
    for (const listener of this.listeners) {
      try {
        listener(s);
      } catch (err) {
        console.error("Font listener error:", err);
      }
    }
  }

  public apply(): void {
    const root = document.documentElement;
    const effectiveFamily = this.getEffectiveFontFamily();
    const isLigatureCapable = this.isCurrentFontLigatureCapable();

    root.style.setProperty("--twominal-font-family", effectiveFamily);
    root.style.setProperty("--twominal-font-size", `${this.settings.fontSize}px`);
    root.style.setProperty("--twominal-font-weight", String(this.settings.fontWeight));
    root.style.setProperty("--twominal-line-height", String(this.settings.lineHeight));
    root.style.setProperty("--twominal-letter-spacing", `${this.settings.letterSpacing}px`);

    const viewport = document.getElementById("terminal-viewport");
    const previewBox = document.getElementById("font-preview-box");
    const inputRendered = document.getElementById("input-rendered");
    const cliInput = document.getElementById("cli-input");

    const enableLig = this.settings.ligatures;

    if (viewport) {
      viewport.classList.toggle("enable-ligatures", enableLig);
      viewport.classList.toggle("disable-ligatures", !enableLig);
    }
    if (previewBox) {
      previewBox.classList.toggle("enable-ligatures", enableLig);
      previewBox.classList.toggle("disable-ligatures", !enableLig);
    }
    if (inputRendered) {
      inputRendered.classList.toggle("enable-ligatures", enableLig);
      inputRendered.classList.toggle("disable-ligatures", !enableLig);
    }
    if (cliInput) {
      cliInput.classList.toggle("enable-ligatures", enableLig);
      cliInput.classList.toggle("disable-ligatures", !enableLig);
    }

    // Status bar labels
    const sbFontInfo = document.getElementById("sb-font-info");
    if (sbFontInfo) {
      const dispName =
        this.settings.fontFamily === "custom" && this.settings.customFamily
          ? this.settings.customFamily
          : this.settings.fontFamily.replace(", Monaco, Consolas, monospace", "");
      sbFontInfo.textContent = `${dispName} ${this.settings.fontSize}px`;
    }

    const sbLigatures = document.getElementById("sb-ligatures-status");
    if (sbLigatures) {
      if (!isLigatureCapable) {
        sbLigatures.textContent = "(no liga in font)";
        sbLigatures.className = "text-[11px] opacity-60 dark:text-slate-400 text-slate-500";
      } else {
        sbLigatures.textContent = this.settings.ligatures ? "(liga: on)" : "(liga: off)";
        sbLigatures.className = this.settings.ligatures
          ? "text-[11px] opacity-90 dark:text-emerald-400 text-emerald-700"
          : "text-[11px] opacity-60 dark:text-slate-400 text-slate-500";
      }
    }

    const checkLigatures = document.getElementById("check-ligatures");
    if (checkLigatures) {
      checkLigatures.textContent = this.settings.ligatures ? "ON" : "OFF";
      checkLigatures.className = this.settings.ligatures
        ? "dark:text-emerald-400 text-emerald-600 font-bold"
        : "text-slate-500 font-bold";
    }

    const labelLigStatus = document.getElementById("label-ligatures-status");
    const btnToggleLig = document.getElementById("btn-toggle-font-ligatures");
    if (labelLigStatus) {
      labelLigStatus.textContent = this.settings.ligatures ? "ON" : "OFF";
    }
    if (btnToggleLig) {
      if (this.settings.ligatures) {
        btnToggleLig.className =
          "px-3 py-1 rounded-md text-xs font-bold transition-colors dark:bg-emerald-500/20 bg-emerald-50 text-emerald-600 dark:text-emerald-400 border border-emerald-500/30";
      } else {
        btnToggleLig.className =
          "px-3 py-1 rounded-md text-xs font-bold transition-colors dark:bg-slate-800 bg-slate-100 text-slate-400 dark:text-slate-500 border border-slate-600/30";
      }
    }

    // Modal sync
    const labelFontSize = document.getElementById("label-font-size-val");
    if (labelFontSize) labelFontSize.textContent = `${this.settings.fontSize}px`;

    const inputFontSize = document.getElementById("input-font-size") as HTMLInputElement | null;
    if (inputFontSize && Number(inputFontSize.value) !== this.settings.fontSize) {
      inputFontSize.value = String(this.settings.fontSize);
    }

    const labelLineHeight = document.getElementById("label-line-height-val");
    if (labelLineHeight) labelLineHeight.textContent = String(this.settings.lineHeight);

    const inputLineHeight = document.getElementById(
      "input-line-height"
    ) as HTMLInputElement | null;
    if (inputLineHeight && Number(inputLineHeight.value) !== this.settings.lineHeight) {
      inputLineHeight.value = String(this.settings.lineHeight);
    }

    const labelLetterSpacing = document.getElementById("label-letter-spacing-val");
    if (labelLetterSpacing)
      labelLetterSpacing.textContent = `${this.settings.letterSpacing}px`;

    const inputLetterSpacing = document.getElementById(
      "input-letter-spacing"
    ) as HTMLInputElement | null;
    if (
      inputLetterSpacing &&
      Number(inputLetterSpacing.value) !== this.settings.letterSpacing
    ) {
      inputLetterSpacing.value = String(this.settings.letterSpacing);
    }

    const selectFamily = document.getElementById(
      "select-font-family"
    ) as HTMLSelectElement | null;
    if (selectFamily && selectFamily.value !== this.settings.fontFamily) {
      selectFamily.value = this.settings.fontFamily;
    }

    const inputCustomFont = document.getElementById(
      "input-custom-font"
    ) as HTMLInputElement | null;
    if (inputCustomFont) {
      inputCustomFont.classList.toggle("hidden", this.settings.fontFamily !== "custom");
      if (this.settings.customFamily && inputCustomFont.value !== this.settings.customFamily) {
        inputCustomFont.value = this.settings.customFamily;
      }
    }

    const selectWeight = document.getElementById(
      "select-font-weight"
    ) as HTMLSelectElement | null;
    if (selectWeight && Number(selectWeight.value) !== this.settings.fontWeight) {
      selectWeight.value = String(this.settings.fontWeight);
    }

    // Ligature compatibility note in modal
    const noteEl = document.getElementById("font-ligature-compatibility-note");
    if (noteEl) {
      const activeFamilyName =
        this.settings.fontFamily === "custom" && this.settings.customFamily
          ? this.settings.customFamily
          : this.settings.fontFamily.replace(", Monaco, Consolas, monospace", "");

      if (isLigatureCapable) {
        if (this.settings.ligatures) {
          noteEl.className =
            "text-[11px] p-2.5 rounded-lg dark:bg-emerald-500/10 bg-emerald-50 text-emerald-700 dark:text-emerald-400 border dark:border-emerald-500/30 border-emerald-200 flex items-center gap-2";
          noteEl.innerHTML = `<span>✓</span><span><b>${activeFamilyName}</b> supports coding ligatures (<code class="font-bold">=== !== =&gt; &lt;= != &lt;!--</code>).</span>`;
        } else {
          noteEl.className =
            "text-[11px] p-2.5 rounded-lg dark:bg-slate-800/60 bg-slate-100 text-slate-400 dark:text-slate-400 border dark:border-slate-700 border-slate-200 flex items-center gap-2";
          noteEl.innerHTML = `<span>ℹ️</span><span>Ligatures are currently <b>Disabled</b>. Toggle ON to enable symbol ligatures.</span>`;
        }
      } else {
        noteEl.className =
          "text-[11px] p-2.5 rounded-lg dark:bg-amber-500/10 bg-amber-50 text-amber-800 dark:text-amber-300 border dark:border-amber-500/30 border-amber-200 flex items-start gap-2";
        noteEl.innerHTML = `<span>⚠️</span><span><b>${activeFamilyName}</b> is a standard monospace font without built-in coding ligatures. To use ligatures like <code class="font-bold">===</code> or <code class="font-bold">=&gt;</code>, switch to <b>Fira Code</b>, <b>JetBrains Mono</b>, <b>Cascadia Code</b>, or <b>Victor Mono</b>.</span>`;
      }
    }
  }
}

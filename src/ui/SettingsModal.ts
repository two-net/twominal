import { FontManager } from "../fonts/FontManager";
import { TabManager, TabTitleFormat } from "../tabs/TabManager";

export class SettingsModal {
  private modalEl: HTMLElement | null;
  private fontManager: FontManager;
  private tabManager?: TabManager;

  constructor(tabManager?: TabManager) {
    this.fontManager = FontManager.getInstance();
    this.tabManager = tabManager;
    this.modalEl = document.getElementById("modal-settings");

    this.bindEvents();
    this.syncFontControls();
  }

  public setTabManager(tabManager: TabManager): void {
    this.tabManager = tabManager;
    this.syncTabFormatSelect();
  }

  private syncTabFormatSelect(): void {
    const select = document.getElementById("select-tab-title-format") as HTMLSelectElement | null;
    if (select && this.tabManager) {
      select.value = this.tabManager.getTitleFormat();
    }
  }

  public syncFontControls(): void {
    const s = this.fontManager.getSettings();

    // Font Family
    const selectFamily = document.getElementById(
      "select-font-family"
    ) as HTMLSelectElement | null;
    const inputCustom = document.getElementById(
      "input-custom-font"
    ) as HTMLInputElement | null;

    if (selectFamily) {
      const match = Array.from(selectFamily.options).some(
        (opt) => opt.value === s.fontFamily
      );
      if (match) {
        selectFamily.value = s.fontFamily;
      } else {
        selectFamily.value = "custom";
      }
    }

    if (inputCustom) {
      const isCustom = selectFamily?.value === "custom" || s.fontFamily === "custom";
      inputCustom.classList.toggle("hidden", !isCustom);
      if (s.customFamily) {
        inputCustom.value = s.customFamily;
      }
    }

    // Font Size
    const inputSize = document.getElementById("input-font-size") as HTMLInputElement | null;
    const labelSize = document.getElementById("label-font-size-val");
    if (inputSize) inputSize.value = String(s.fontSize);
    if (labelSize) labelSize.textContent = `${s.fontSize}px`;

    // Font Weight
    const selectWeight = document.getElementById(
      "select-font-weight"
    ) as HTMLSelectElement | null;
    if (selectWeight) selectWeight.value = String(s.fontWeight);

    // Line Height
    const inputLineHeight = document.getElementById(
      "input-line-height"
    ) as HTMLInputElement | null;
    const labelLineHeight = document.getElementById("label-line-height-val");
    if (inputLineHeight) inputLineHeight.value = String(s.lineHeight);
    if (labelLineHeight) labelLineHeight.textContent = String(s.lineHeight);

    // Letter Spacing
    const inputSpacing = document.getElementById(
      "input-letter-spacing"
    ) as HTMLInputElement | null;
    const labelSpacing = document.getElementById("label-letter-spacing-val");
    if (inputSpacing) inputSpacing.value = String(s.letterSpacing);
    if (labelSpacing) labelSpacing.textContent = `${s.letterSpacing}px`;

    // Ligatures
    const labelLig = document.getElementById("label-ligatures-status");
    const btnToggleLig = document.getElementById("btn-toggle-font-ligatures");
    if (labelLig) {
      labelLig.textContent = s.ligatures ? "ON" : "OFF";
    }
    if (btnToggleLig) {
      if (s.ligatures) {
        btnToggleLig.className =
          "px-3 py-1 rounded-md text-xs font-bold transition-colors dark:bg-emerald-500/20 bg-emerald-50 text-emerald-600 dark:text-emerald-400 border border-emerald-500/30";
      } else {
        btnToggleLig.className =
          "px-3 py-1 rounded-md text-xs font-bold transition-colors dark:bg-slate-800 bg-slate-100 text-slate-400 dark:text-slate-500 border border-slate-600/30";
      }
    }

    // Preview box
    const previewBox = document.getElementById("font-preview-box");
    if (previewBox) {
      previewBox.classList.toggle("enable-ligatures", s.ligatures);
      previewBox.classList.toggle("disable-ligatures", !s.ligatures);
    }
  }

  public open(): void {
    if (this.modalEl) {
      this.syncTabFormatSelect();
      this.syncFontControls();
      this.modalEl.classList.remove("hidden");
    }
  }

  public close(): void {
    if (this.modalEl) {
      this.modalEl.classList.add("hidden");
      document.getElementById("cli-input")?.focus();
    }
  }

  public toggle(): void {
    if (this.modalEl?.classList.contains("hidden")) {
      this.open();
    } else {
      this.close();
    }
  }

  private bindEvents(): void {
    document.getElementById("btn-close-settings")?.addEventListener("click", () => this.close());
    document.getElementById("btn-settings-got-it")?.addEventListener("click", () => this.close());

    const selectTabFormat = document.getElementById(
      "select-tab-title-format"
    ) as HTMLSelectElement | null;
    selectTabFormat?.addEventListener("change", () => {
      if (this.tabManager && selectTabFormat.value) {
        this.tabManager.setTitleFormat(selectTabFormat.value as TabTitleFormat);
      }
    });

    // 1. Font Family
    const selectFamily = document.getElementById(
      "select-font-family"
    ) as HTMLSelectElement | null;
    const inputCustom = document.getElementById(
      "input-custom-font"
    ) as HTMLInputElement | null;

    selectFamily?.addEventListener("change", () => {
      const val = selectFamily.value;
      if (val === "custom") {
        inputCustom?.classList.remove("hidden");
        inputCustom?.focus();
        if (inputCustom?.value.trim()) {
          this.fontManager.setFontFamily("custom", inputCustom.value.trim());
        } else {
          this.fontManager.setFontFamily("custom");
        }
      } else {
        inputCustom?.classList.add("hidden");
        this.fontManager.setFontFamily(val);
      }
    });

    inputCustom?.addEventListener("input", () => {
      if (selectFamily?.value === "custom") {
        this.fontManager.setFontFamily("custom", inputCustom.value.trim());
      }
    });

    // 2. Font Size
    const inputSize = document.getElementById("input-font-size") as HTMLInputElement | null;
    inputSize?.addEventListener("input", () => {
      const val = parseInt(inputSize.value, 10);
      if (!isNaN(val)) {
        this.fontManager.setFontSize(val);
      }
    });

    document.getElementById("btn-font-dec")?.addEventListener("click", () => {
      this.fontManager.decreaseFontSize(1);
    });

    document.getElementById("btn-font-inc")?.addEventListener("click", () => {
      this.fontManager.increaseFontSize(1);
    });

    // 3. Font Weight
    const selectWeight = document.getElementById(
      "select-font-weight"
    ) as HTMLSelectElement | null;
    selectWeight?.addEventListener("change", () => {
      const val = parseInt(selectWeight.value, 10);
      if (!isNaN(val)) {
        this.fontManager.setFontWeight(val);
      }
    });

    // 4. Line Height
    const inputLineHeight = document.getElementById(
      "input-line-height"
    ) as HTMLInputElement | null;
    inputLineHeight?.addEventListener("input", () => {
      const val = parseFloat(inputLineHeight.value);
      if (!isNaN(val)) {
        this.fontManager.setLineHeight(val);
      }
    });

    // 5. Letter Spacing
    const inputSpacing = document.getElementById(
      "input-letter-spacing"
    ) as HTMLInputElement | null;
    inputSpacing?.addEventListener("input", () => {
      const val = parseFloat(inputSpacing.value);
      if (!isNaN(val)) {
        this.fontManager.setLetterSpacing(val);
      }
    });

    // 6. Ligatures toggle
    document.getElementById("btn-toggle-font-ligatures")?.addEventListener("click", () => {
      this.fontManager.toggleLigatures();
    });

    // 7. Reset Defaults
    document.getElementById("btn-font-reset")?.addEventListener("click", () => {
      this.fontManager.resetDefaults();
    });

    // Font change listener to keep UI elements synced
    this.fontManager.onFontChange(() => {
      this.syncFontControls();
    });

    // Modal background click
    this.modalEl?.addEventListener("click", (e) => {
      if (e.target === this.modalEl) {
        this.close();
      }
    });
  }
}

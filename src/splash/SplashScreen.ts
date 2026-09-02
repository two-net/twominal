export class SplashScreen {
  private splashScreenEl: HTMLElement | null;
  private progressEl: HTMLElement | null;
  private statusEl: HTMLElement | null;

  constructor() {
    this.splashScreenEl = document.getElementById("splash-screen");
    this.progressEl = document.getElementById("splash-progress");
    this.statusEl = document.getElementById("splash-status");
  }

  public async animateSequence(): Promise<void> {
    const steps = [
      { progress: "30%", text: "Mounting Virtual POSIX PTY..." },
      { progress: "65%", text: "Initializing Fish autosuggestion parser..." },
      { progress: "90%", text: "Loading Fira Code font ligatures..." },
      { progress: "100%", text: "Twominal ready." },
    ];

    for (let i = 0; i < steps.length; i++) {
      if (this.progressEl) this.progressEl.style.width = steps[i].progress;
      if (this.statusEl) this.statusEl.textContent = steps[i].text;
      await new Promise((res) => setTimeout(res, 200));
    }
  }

  public async hide(delayMs: number = 300): Promise<void> {
    await new Promise((resolve) => setTimeout(resolve, delayMs));
    if (this.splashScreenEl) {
      this.splashScreenEl.classList.add("opacity-0", "pointer-events-none");
      setTimeout(() => {
        if (this.splashScreenEl) {
          this.splashScreenEl.style.display = "none";
        }
      }, 700);
    }
  }
}

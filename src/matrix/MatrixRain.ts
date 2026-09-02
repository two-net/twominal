export class MatrixRain {
  private canvas: HTMLCanvasElement;
  private ctx: CanvasRenderingContext2D | null = null;
  private isActive: boolean = false;
  private animationInterval: number | null = null;
  private drops: number[] = [];
  private characters = "01TWOMINALFISHPTY{}[]<>/\\$#@!*+-=~";
  private fontSize = 14;

  constructor(canvas: HTMLCanvasElement) {
    this.canvas = canvas;
    this.ctx = canvas?.getContext("2d") || null;
    this.handleResize = this.handleResize.bind(this);
    window.addEventListener("resize", this.handleResize);
  }

  public toggle(): boolean {
    if (this.isActive) {
      this.stop();
    } else {
      this.start();
    }
    return this.isActive;
  }

  public getIsActive(): boolean {
    return this.isActive;
  }

  public start(): void {
    if (this.isActive || !this.ctx || !this.canvas) return;
    this.isActive = true;
    this.canvas.style.display = "block";
    this.handleResize();

    const columns = Math.floor(this.canvas.width / this.fontSize);
    this.drops = Array(columns).fill(1);

    if (this.animationInterval) {
      clearInterval(this.animationInterval);
    }
    this.animationInterval = window.setInterval(() => this.draw(), 45);
  }

  public stop(): void {
    this.isActive = false;
    if (this.canvas) {
      this.canvas.style.display = "none";
    }
    if (this.animationInterval) {
      clearInterval(this.animationInterval);
      this.animationInterval = null;
    }
    if (this.ctx && this.canvas) {
      this.ctx.clearRect(0, 0, this.canvas.width, this.canvas.height);
    }
  }

  private handleResize(): void {
    if (!this.canvas || !this.canvas.parentElement) return;
    this.canvas.width = this.canvas.parentElement.offsetWidth;
    this.canvas.height = this.canvas.parentElement.offsetHeight;
    const columns = Math.floor(this.canvas.width / this.fontSize);
    this.drops = Array(columns).fill(1);
  }

  private draw(): void {
    if (!this.ctx || !this.isActive || !this.canvas) return;

    this.ctx.fillStyle = "rgba(13, 17, 23, 0.1)";
    this.ctx.fillRect(0, 0, this.canvas.width, this.canvas.height);
    this.ctx.fillStyle = "#50fa7b";
    this.ctx.font = `${this.fontSize}px monospace`;

    for (let i = 0; i < this.drops.length; i++) {
      const char = this.characters[Math.floor(Math.random() * this.characters.length)];
      this.ctx.fillText(char, i * this.fontSize, this.drops[i] * this.fontSize);

      if (this.drops[i] * this.fontSize > this.canvas.height && Math.random() > 0.975) {
        this.drops[i] = 0;
      }
      this.drops[i]++;
    }
  }

  public dispose(): void {
    this.stop();
    window.removeEventListener("resize", this.handleResize);
  }
}

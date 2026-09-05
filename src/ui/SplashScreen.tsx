interface SplashScreenProps {
  visible: boolean;
}

export function SplashScreen({ visible }: SplashScreenProps) {
  return (
    <div
      className={`splash-screen ${visible ? "is-visible" : "is-hidden"}`}
      role="status"
      aria-label="Twominal is starting"
      aria-hidden={!visible}
    >
      <span className="splash-brand">
        <span className="splash-mark" aria-hidden="true">
          &gt;_
        </span>
        <span className="splash-wordmark">Twominal</span>
        <span className="splash-tagline">terminal, twice as friendly</span>
      </span>
    </div>
  );
}

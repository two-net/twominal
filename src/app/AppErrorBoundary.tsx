import { Component, type ReactNode } from "react";

interface AppErrorBoundaryProps {
  children: ReactNode;
  onReload?: () => void;
}

interface AppErrorBoundaryState {
  failed: boolean;
}

export class AppErrorBoundary extends Component<
  AppErrorBoundaryProps,
  AppErrorBoundaryState
> {
  state: AppErrorBoundaryState = { failed: false };

  static getDerivedStateFromError(): AppErrorBoundaryState {
    return { failed: true };
  }

  private readonly reload = (): void => {
    if (this.props.onReload) {
      this.props.onReload();
      return;
    }

    window.location.reload();
  };

  render(): ReactNode {
    if (!this.state.failed) {
      return this.props.children;
    }

    return (
      <main className="fatal-error-screen">
        <section
          className="fatal-error-card"
          role="alert"
          aria-labelledby="fatal-error-title"
          aria-describedby="fatal-error-description"
        >
          <p className="fatal-error-brand" aria-hidden="true">
            TWOMINAL
          </p>
          <h1 id="fatal-error-title">Twominal needs to reload</h1>
          <p id="fatal-error-description">
            This window encountered an unexpected problem. Reload it to start a
            fresh terminal workspace.
          </p>
          <button
            className="fatal-error-reload"
            type="button"
            onClick={this.reload}
          >
            Reload Twominal
          </button>
        </section>
      </main>
    );
  }
}

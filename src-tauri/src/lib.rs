//! Twominal's app crate: the App Shell and the stores and managers it owns (SRS §6.1).
//!
//! One module per component. DESIGN.md §1 lists them all and says where the tab components
//! and the webview code live.

mod app;
mod history;
mod settings;
mod tabs;
mod theme;

pub fn run() {
    app::run()
}

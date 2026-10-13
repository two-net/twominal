// App Shell's page (SRS §6.1). Its Rust half is src-tauri/src/app.rs.
//
// The page every window loads: it hosts the tab bar, the terminal canvas, the splash
// screen and the settings, About and help screens.
//
// Must not contain terminal logic, and holds no state of its own: it shows what Rust
// sends and reports what the user does.
//
// This is the only webview file that touches window.__TAURI__. Every other module is
// handed what it needs from here, so none of them depends on the windowing framework
// (ARC-02). DESIGN.md §1 lists the other modules and where each one goes.

const { invoke } = window.__TAURI__.core;

invoke("app_ready");

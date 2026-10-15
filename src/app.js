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
const { getCurrentWebviewWindow } = window.__TAURI__.webviewWindow;

// Notices that belong to no tab, such as a bad value in the configuration file (SET-05).
// Rust sends the whole list each time, so the page shows the latest and keeps nothing.
const notices = document.getElementById("notices");
const list = notices.querySelector("ul");

function showNotices(texts) {
  // The same list again, as when another setting changed, leaves a dismissed list hidden.
  const shown = [...list.children].map((item) => item.textContent);
  if (texts.length === shown.length && texts.every((text, i) => text === shown[i])) {
    return;
  }
  // The texts quote the configuration file, so they are never read as HTML.
  list.replaceChildren(
    ...texts.map((text) => {
      const item = document.createElement("li");
      item.textContent = text;
      return item;
    }),
  );
  notices.hidden = texts.length === 0;
}

// Dismissing hides the list until Rust sends another.
notices.querySelector("button").addEventListener("click", () => {
  notices.hidden = true;
});

// The font the terminal area draws with, from the Font and Font size settings: a CSS family
// list and its size in CSS pixels (FONT-02). #31 gives it to the Renderer it creates there,
// with `setFont`, and gives it each new one.
let font = null;

// Listening comes first: app_ready makes Rust send what it found before this page loaded,
// and the font. The listeners are this window's own, so they hear that answer and what goes
// to every window.
const current = getCurrentWebviewWindow();
Promise.all([
  current.listen("settings:notices", (event) => showNotices(event.payload)),
  current.listen("settings:font", (event) => {
    const [family, size] = event.payload;
    font = { family, size };
  }),
]).then(() => invoke("app_ready"));

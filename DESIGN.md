# Twominal — Design

**Version:** 0.16 · **Date:** 2026-10-15 · **Tracks:** [SRS](SRS.md) v0.6

> The SRS says what Twominal must do. This document says how it is built: the technology choices the SRS leaves open (C-1) and where each component of SRS §6 lives. Requirement IDs such as `ARC-02` are the SRS's. Where the two disagree, the SRS wins and this document is corrected.

## Contents

1. [Overview](#1-overview)
2. [Technology choices](#2-technology-choices)
3. [Components](#3-components)
4. [A tab at run time](#4-a-tab-at-run-time)
5. [Cross-cutting concerns](#5-cross-cutting-concerns)
6. [What 1.0 does not contain](#6-what-10-does-not-contain)
7. [Files and formats](#7-files-and-formats)
8. [Testing](#8-testing)
9. [Risks](#9-risks)
10. [Requirements this document answers](#10-requirements-this-document-answers)
11. [Changing this document](#11-changing-this-document)

## 1. Overview

Twominal is one native process written in Rust, plus one OS webview per window. All state and logic are in Rust. The webview draws what it is sent, captures raw input and runs animations.

```
┌───────────────────────── Twominal process (Rust) ─────────────────────────┐
│ App Shell ──► Settings Store · History Store · Theme Manager              │
│     │                                                                     │
│     └─ per window ─► Tab Manager ─ handles ─► Tab, one thread each        │
│                                               ├─ Session ◄─ PTY ─► shell  │
│                                               ├─ Terminal Core            │
│                                               ├─ Command Line             │
│                                               ├─ Input Handler            │
│                                               └─ Renderer                 │
└──────────────▲─────────────────────────────────────────┬──────────────────┘
     input events, commands                    frames, change events
┌──────────────┴─────────────────────────────────────────▼──────────────────┐
│ Webview, one per window: App Shell's page · Animator · tab bar · canvas   │
└───────────────────────────────────────────────────────────────────────────┘
```

Each component of SRS §6.1 has one home. Tier 1 is the App Shell, tier 2 the stores and managers, tier 3 the tab components, and tier 4 the Terminal Core, which the other tab components read and which names nothing.

<!-- components:begin -->
| Component | Tier | Rust home | Webview home | Interface |
|---|---|---|---|---|
| App Shell | 1 | `src-tauri/src/app.rs` | `src/app.js` | Every command from the webview and every event to it |
| Settings Store | 2 | `src-tauri/src/settings.rs` | — | Hands out settings; announces each change |
| History Store | 2 | `src-tauri/src/history.rs` | — | Implements the history trait the Command Line defines |
| Theme Manager | 2 | `src-tauri/src/theme.rs` | `src/theme.js` | Theme setting and OS appearance in; the palette out |
| Animator | 2 | — | `src/animator.js` | Runs an animation a view asks for; decides how, or whether |
| Tab Manager | 2 | `src-tauri/src/tabs.rs` | `src/tabs.js` | Opens, orders, renames and closes tabs through their handles |
| Tab | 3 | `crates/tab/src/lib.rs` | — | `Message` in through its `Handle`; frames and notices out |
| Session | 3 | `crates/tab/src/session.rs` | — | Bytes to and from the shell; the exit status |
| Terminal Core | 4 | `crates/terminal-core/src/lib.rs` | — | Bytes in; screen state, replies and shell reports out |
| Command Line | 3 | `crates/tab/src/command_line.rs` | — | Keys in; the finished line and requests for slow work out |
| Input Handler | 3 | `crates/tab/src/input.rs` | `src/tab/input.js` | `InputEvent` in; one action out |
| Renderer | 3 | `crates/tab/src/render.rs` | `src/tab/renderer.js` | Reads the Terminal Core and Command Line; `Frame` out |
<!-- components:end -->

**Dependency rule (ARC-02).** Code names only what is in a lower tier, and never a sibling in its own tier.

- Cargo enforces the tiers. The app depends on `tab`, `tab` depends on `terminal-core`, and nothing points back.
- Wiring happens in two places only: the App Shell wires the stores and managers, and the Tab wires the tab components.
- What must travel upward does so as a return value, or through something handed down when the lower part was created.
- Only the App Shell names Tauri: `app.rs` in Rust and `app.js` in the webview.
- The Terminal Core is `no_std`. It cannot name a process, file, socket, thread or clock.

**Between the process and a webview.** The webview calls Tauri commands. Rust answers with events, and with one channel per attached tab for frames. Both ends are the App Shell.

The table above and the dependency table in §2 are read by `src-tauri/tests/architecture.rs`, which fails when the code and this page disagree (ARC-07).

## 2. Technology choices

| Concern | Choice | Serves |
|---|---|---|
| App framework | Tauri v2: one Rust process, one OS webview per window | PORT-01, UI-05 |
| Core language | Rust, stable toolchain | REL-02, PERF-06, ARC-03 |
| Webview code | Plain HTML, CSS and ES modules, using the global `window.__TAURI__`. No bundler, no npm | ARC-06, C-3 |
| Where logic lives | All state and logic in Rust. The webview holds no model state | REL-01, PERF-07, TAB-15 |
| Terminal Core | Our own screen model on the `vte` parser | TERM-01, SEC-05, ARC-03, MAINT-03 |
| Fuzzing | `cargo-fuzz` and libFuzzer, on the nightly toolchain, for the Terminal Core. Only the fuzz crate needs nightly, and it is not part of what ships | REL-02 |
| Renderer | Canvas 2D in device pixels. Each row is drawn as runs of same-style text, each character at its own cell, and lines, blocks and powerline symbols as shapes | FONT-04 to FONT-11, SW-06, HW-03, ANIM-05 |
| Pseudo-terminal | `portable-pty`, which uses ConPTY on Windows | SW-01, PORT-01 |
| Rust ⇄ webview | Tauri commands in; events and channels out | PERF-04, PERF-07 |
| Settings file | TOML | SET-03, SET-04 |
| Watching the settings file | One thread asks the OS for the file's modification time, four times a second. No watcher library | SET-04, ARC-06 |
| Launcher hand-over | One Unix socket in a folder only the user can open, made with the standard library. No single-instance library | GEN-05, ARC-06, SEC-01 |
| Animation | Web Animations API, started only by the Animator | ANIM-01 to ANIM-04, ARC-05 |
| OS appearance | Tauri's window theme events | SW-03, THM-02 |
| OS reduced motion | The `prefers-reduced-motion` media query | SW-04, ACC-01 |

### Why Tauri v2 and a plain HTML frontend stay

The repository was reset to an empty Tauri v2 scaffold with a plain HTML frontend. Both stay.

- **Three OSes from one codebase.** Tauri gives windows, native menus and a webview on macOS, Linux and Windows (PORT-01, UI-05). It uses the OS's webview instead of shipping a browser engine. On Linux that is WebKitGTK, which a package installs as a dependency.
- **The webview's text engine does the hard part of drawing.** Canvas text is shaped by the webview, so every ligature a font has, any installed font and fallback fonts work without a text stack of our own (FONT-04, FONT-11, SW-06). Where a run of text starts and ends is ours to choose, and that is all the ligature rules need (FONT-05 to FONT-08).
- **No build step for the webview.** `src/` is served as it is. Building Twominal downloads Rust crates and nothing else, so there is no JavaScript dependency to audit (ARC-06, C-3).
- **v0.1.x shows where not to go.** It ran the terminal in the webview on xterm.js from npm. That made the terminal core hard to test without a window (ARC-03) and impossible to fuzz with Rust's tools (REL-02), and it needed a bundler.

### Why the logic is in Rust

- A tab that floods output works on its own thread and cannot block the interface (REL-01, PERF-07).
- The Terminal Core, the Command Line and key handling are tested and fuzzed with `cargo`, with no window (ARC-03, MAINT-03, REL-02).
- Several windows share the stores without sharing a webview (TAB-15, CMD-12).
- The cost is one IPC message per key and one per frame. §9 says how that is kept inside PERF-04.

### Turned down

| Alternative | Why not |
|---|---|
| Terminal Core compiled to WebAssembly, running in the webview | Adds a build step, and every byte of output would cross IPC before it is parsed |
| Terminal logic in JavaScript, as in v0.1.x | Without npm it would be hand-written JavaScript, hard to fuzz and to keep at 25 MB/s (PERF-06) |
| `alacritty_terminal` for the whole terminal model | Carries its own pseudo-terminal and event-loop code, against ARC-02, and follows Alacritty's needs |
| A hand-written parser | `vte` is small, widely used and built for hostile input (REL-02) |
| WebGL2 glyph atlas | Ligature shaping and font fallback become our code. Kept as the fallback in §9 |
| One DOM element per row | Weakest on frame rate and on a pixel-exact grid (PERF-05, FONT-05) |
| `tauri-plugin-single-instance` for the launcher | A dependency, and `zbus` with it on Linux. On macOS its socket is in the shared `/tmp` under a name anyone can guess. The second process starts Tauri before it hands over |

### Direct dependencies

Every crate Twominal depends on directly is listed here, and nothing else may be (ARC-06). Add the row first, then the dependency.

<!-- dependencies:begin -->
| Crate | Depends on | For |
|---|---|---|
| `twominal` | `tauri` | Windows, menus, the webview and IPC |
| `twominal` | `tauri-build` | The build script `tauri` needs |
| `twominal` | `tab` | The tab tier |
| `twominal` | `toml_edit` | Reads the configuration file, and saves it with the user's comments kept (SET-03, SET-04) |
| `twominal` | `fontdb` | Finds the installed fonts: the OS's font folders, and fontconfig's configuration on Linux, so that a Font setting that names none is reported (SET-05, SW-06). Default features on: `memmap` maps each file, which is `unsafe`, so that only the name tables of large fonts are read. Without it Apple Color Emoji's 180 MB would be read whole |
| `twominal` | `ttf-parser` | Reads each installed face's family names, of which fontdb keeps one (SET-05). Default features off but `std` |
| `tab` | `terminal-core` | The Terminal Core |
| `tab` | `portable-pty` | Pseudo-terminals: starts the shell on one and carries its bytes, through ConPTY on Windows (SW-01, PORT-01) |
| `terminal-core` | `vte` | Parses escape sequences. Default features off: no `std`, and a bounded buffer for OSC sequences (ARC-02, REL-02) |
| `terminal-core` | `unicode-width` | How many cells a character takes, by UAX #11 (TERM-02). Default features off: no CJK tables, so ambiguous characters are narrow, as in xterm. Pinned to 0.2.2, the version `width.rs` was compared with |
| `terminal-core-fuzz` | `terminal-core` | The crate it fuzzes |
| `terminal-core-fuzz` | `libfuzzer-sys` | Runs the fuzz targets under libFuzzer (REL-02). The fuzz crate is outside the workspace and is not part of what ships |
<!-- dependencies:end -->

Planned, and added by the issue that first needs each:

| Crate | Will depend on | For | Issue |
|---|---|---|---|
| `twominal` | A clipboard library that reaches the Linux primary selection | SW-05 | #28 |

## 3. Components

As of this version the Settings Store, the Session, the Terminal Core, the Rust half of the Input Handler and both halves of the Renderer are built, and the App Shell opens windows, builds the native menus, gives the product its name and logo, gives the pages their font and answers the `twominal` command. Every other Rust home exists as a skeleton: types that say who owns what, and no behaviour. Of the webview homes `src/app.js` and `src/tab/renderer.js` exist. The others are created by their issues, at the paths in §1. No window shows a tab yet: #31 attaches one, and wires the Renderer's two halves together.

### Tier 1: App Shell

- **Owns** the stores, the Theme Manager, every window, the native menus and the splash screen. Each window has one Tab Manager.
- **Is the only code that names Tauri.** Every command and event is defined in `app.rs`. Commands are named `<component>_<verb>`, such as `app_ready` or `tabs_open`, and events `<component>:<what>`.
- **Wires the stores and managers.** It hears a settings change and passes it to the Theme Manager, the Tab Managers and the webviews. They never name each other.
- **Creates windows in code, not from `tauri.conf.json`.** A window then opens with the settings it needs for its first frame, so there is no flash of the wrong theme (SPL-02) and reduced motion applies from the start (SPL-07). So far that is the OS appearance the Theme setting asks for. The resolved background comes with #39 and the splash with #41.
- **Its page** hosts the tab bar, the canvas, the splash screen and the settings, About and help screens (UI-03). The tab bar is at the top and the terminal area fills the rest. Showing or hiding the tab bar takes room from the terminal area and never from the window (UI-01).
- **Shows the notices that belong to no tab**, such as a bad value in the configuration file (SET-05). They overlay the page and take no room from the terminal (UI-02). A list the same as the one shown changes nothing, so a list the user dismissed stays hidden when a setting changes.
- **Must not** contain terminal logic.

#### Windows and menus

- **Every window is alike.** There is no main window. A window's label is `window-1`, `window-2` and so on, and a label is never reused. The capability in `src-tauri/capabilities/default.json` names them all as `window-*`.
- **The title** is the active tab's title, which the window's Tab Manager gives, or the product's name while no tab has one (GEN-04).
- **A new window** (TAB-15) asked for from a menu is opened from a thread of its own, because on Windows the webview cannot be created from inside an event handler. On macOS it is placed a step down and right of the focused window, or of the newest one when Twominal is not in front, as when the launcher asks. Elsewhere the OS places it.
- **The last window.** On macOS Twominal keeps running with no window, and a click on its Dock icon opens one. On Linux and Windows closing the last window quits (TAB-07).
- **The theme.** A theme chosen in the View menu is written through the Settings Store (THM-07). The App Shell hears that change as it hears any other, moves the check mark and gives every window the OS appearance the setting asks for: dark, light, or whatever the OS has. The palette is the Theme Manager's (#38).

The menus hold only what works. Each feature adds its own items (UI-05).

| Menu | macOS | Linux and Windows |
|---|---|---|
| Twominal | About, Services, Hide, Hide Others, Show All, Quit | — |
| File | New Window, Close Window | New Window, Close Window, Quit |
| Edit | Undo, Redo, Cut, Copy, Paste, Select All | — |
| View | Theme: Dark, Light, System. Increase Font Size, Decrease Font Size, Reset Font Size. Enter Full Screen | Theme: Dark, Light, System. Increase Font Size, Decrease Font Size, Reset Font Size |
| Window | Minimize, Zoom, Bring All to Front, and the open windows | — |
| Help | The search field | About |

- **On macOS** the menu bar belongs to the app. On Linux and Windows each window shows it.
- **Off macOS no item takes a plain Ctrl+letter** (Appendix A). The OS's ready-made Edit items would take Ctrl+C from the shell, so there the Edit menu waits for #28.
- **Off macOS, Close Window and Quit are Twominal's own items**, because Linux has no ready-made ones.
- **The font size items** take Appendix A's shortcuts: Cmd+=, Cmd+- and Cmd+0 on macOS, and Ctrl+Shift+=, Ctrl+Shift+- and Ctrl+Shift+0 elsewhere (*Fonts*).

#### Launcher

The `twominal` command is the app's own executable (GEN-02, GEN-05). `src-tauri/src/app/launcher.rs` reads its arguments, and `src-tauri/src/app/socket.rs` hands a request to a Twominal that is already running.

| Command | Does |
|---|---|
| `twominal` | Opens a new window. Its first tab starts in the home directory (FISH-01) |
| `twominal DIRECTORY` | The same, with the first tab in that directory |
| `twominal -h`, `--help` | Prints the usage |
| `twominal -V`, `--version` | Prints the name and the version, the ones the About dialog shows |

- **Arguments are read before Tauri starts.** So `--help`, `--version` and every problem need no display, and a command that hands over never shows a second Dock icon.
- **The directory** is made absolute by the process that was given it, which knows what a relative path is relative to. No symbolic link is resolved, so the path stays as it was typed. With no directory the first tab starts in the home directory, not where the command was run. `twominal .` asks for that.
- **A problem** is one line on the standard error, and no window opens. A path that is missing or not a directory ends with status 1. An option Twominal does not know, or a second directory, ends with status 2. `--` ends the options.
- **One process.** If Twominal is running, the command hands it the directory and ends with status 0. The running Twominal opens the window and brings it to the front. If none is running, the command becomes the app and stays in the foreground, like any app started from a shell.
- **The hand-over** is one Unix socket, made with the standard library (ARC-06). §7 says where it is.
  - The folder must be one that only the user can open. With none, nothing is handed over and every command is its own process.
  - Who is first is settled by a lock on a file beside the socket, held for as long as the process lives. The OS frees it when the process ends, however it ends. Only the holder removes an old socket and listens, so two commands started together cannot both become the app.
  - The command sends one byte for the version of this exchange and then the directory, and waits for one byte back. The running Twominal opens the window only after that byte is sent, and a command that gets no answer in two seconds runs on its own. So one command opens one window.
  - Only the directory travels. The window's shell gets the running Twominal's environment, not the caller's.
  - A debug build has a socket of its own, so `cargo tauri dev` and an installed Twominal never answer for each other.
- **The Tab Manager is given the directory** when the window is created, and starts the first tab there (#31).
- **Not on Windows yet.** The standard library cannot listen on a named pipe, so there each command is its own process. A release build has no console there either, so `--help` prints nowhere. Both wait for the first Windows build.

#### Identity

- **The name** is Twominal wherever the user reads it (GEN-01). It is written once, as `productName` in `tauri.conf.json`. The App Shell reads it from Tauri and gives it to the window titles, the menus and the About dialog. The installers take it from the same place.
- **`twominal`**, in lowercase, is every identifier (GEN-02): the package and so the executable, the folders in §7, and the last part of the bundle identifier `com.two.twominal`.
- **The logo** is one file, `src/logo.svg`: a 2 and a block cursor on a dark tile (GEN-03).
  - The tile is the logo's own background, so the mark reads on a dark page and on a light one.
  - The app icons in `src-tauri/icons/` are generated from it with `cargo tauri icon src/logo.svg`. They are never edited by hand.
  - The page shows the file itself, on the splash screen (#41) and the About screen (#44). Until #44 the About dialog is the OS's, and it shows the logo where that dialog has a place for one: on macOS and Linux.
  - It was drawn for Twominal and holds no one else's artwork (C-3).

#### Fonts

`src-tauri/src/app/fonts.rs` turns the Font and Font size settings into the font every page draws with, and finds out whether the font is installed (FONT-02, SW-06, SET-05). The Renderer measures and paints whatever it is given (*Renderer*, Tier 3).

- **The bundled font (FONT-01).** JetBrains Mono 2.304 in its regular, bold, italic and bold italic faces, as WOFF2 in `src/fonts/` with its license (§7). `src/app.css` names the four faces "JetBrains Mono" with `@font-face`. That name hides a copy the user has installed, so every machine draws the same version, with all four faces and the same ligatures. The webview loads a face when it is first asked for.
- **The family** is a CSS list: the Font setting, then "JetBrains Mono", then `monospace`. When the setting is the bundled name, in any letter case, the list starts at "JetBrains Mono". Each name is written as a CSS string with its quotes, backslashes and control characters escaped, since a canvas ignores a `font` it cannot read.
  - A font that is not installed is passed over, so the bundled font is drawn: the default SET-05 asks for.
  - A character the font lacks is drawn from the next font that has it: JetBrains Mono, then the OS's monospace font, then whatever the OS falls back to (FONT-11, SW-06). `monospace` also draws the text if the bundled font could not load. How a fallback glyph fits its cells is the Renderer's.
  - An installed font that is not monospaced is drawn too, each character in its cell.
- **The size.** The setting is in points, and the page is given CSS pixels. A point is a pixel on macOS, which counts 72 to the inch, and 96/72 of one on Linux and Windows, which count 96, as in WezTerm. So 13 pt is 13 px on macOS and 17⅓ px elsewhere: what each OS's own terminals mean by 13 pt.
- **To the pages.** The event `settings:font` carries the family and the size. Every page is sent it when either setting changes, and a page that has just loaded is sent it in answer to `app_ready`. The page hands it to the Renderer (#31), which measures again. The new layout sends every row, so a new font loses nothing on the screen (REL-03), though a smaller grid cuts rows until the Terminal Core reflows them (#26).
- **Is it installed? (SET-05)** While the Font setting is not the bundled name, a thread named `fonts` lists the installed faces with fontdb, which reads the OS's font folders and, on Linux, fontconfig's configuration. It reads each face's family names with ttf-parser: name IDs 1, 16 and 21, in every language, which are the names a CSS family is matched with. When no face has the name, letter case aside, a notice says so: `font: "Nope" is not an installed font. Using "JetBrains Mono".`
  - It runs when Twominal starts and each time the Font setting changes to another name, so a font installed while Twominal runs is found. While the bundled font is chosen, nothing is read.
  - Drawing never waits for it: the family list already falls back to the bundled font. Only the notice waits.
  - The notice is shown with the Settings Store's. A change of font takes it away at once, and a list finished for a font that is no longer chosen is dropped. Choosing the notice and sending the list happen under one lock, so a list cannot overtake a newer one.
- **Zoom (FONT-02).** View ▸ Increase Font Size, Decrease Font Size and Reset Font Size change the Font size setting through the Settings Store, as the theme items do, so every window follows and the size is saved. Increase goes to the next whole point up and Decrease to the next one down, between 6 and 72, and Reset to 13. At a limit the item does nothing. It is not disabled, since a disabled item's key would go on to the program.

### Tier 2: stores and managers

- **Settings Store.** Owns the file, its format, every default in Appendix B and every validation rule. Names no other component.
  - The App Shell creates it with two things: the OS configuration folder, and a function to call with each change. That is how a change travels up without the store naming who hears it.
  - A change carries the settings now, the settings before, and the notices. It is announced the same way whether it came from the settings screen or from a hand edit of the file (SET-02, SET-04).
  - Every setting is written once, in one list: its key in the file, its rule and its field. Loading, saving and comparing all read that list.
  - It watches the file from one thread of its own, and reloads when a save has settled.
  - It never writes over a file it could not read. A change made while the file is broken applies, is not saved, and says so in a notice.
- **History Store.** Owns the shared command history and its file. It implements the history trait that the Command Line defines, and tabs hold it only as that trait (ARC-04). Must not store terminal output.
- **Theme Manager.** Takes the Theme setting and the OS appearance, both passed in by the App Shell, and announces the palette. `src/theme.js` applies the announced palette to the page.
- **Animator.** Lives in the webview, because that is where animations run and where the webview reports the OS reduced-motion setting. Views ask it to animate, and it decides how, or whether. Must not delay input or output.
- **Tab Manager.** One per window. Of each tab it holds a `tab::Handle` and nothing else, so it cannot reach into a tab's session or screen. It is created with the directory its first tab starts in (FISH-01). `src/tabs.js` draws the tab bar.

### Tier 3: a tab

- **Tab.** Owns one Session, Terminal Core, Command Line, Input Handler and Renderer (TAB-01), and is the only code that passes things between them. It is created with one bundle, `tab::Host`, holding everything it may ask of the layers above: the history trait, the clipboard, opening links, and where to send frames and notices. The bundle is added with the first feature that needs it.
- **Session.** Starts the shell on a pseudo-terminal and carries bytes. Holds the shell integration for each supported shell. Must not interpret output.
- **Command Line.** A state machine: keys in, an edited line out. Must not take input in passthrough or depend on fish. When a command runs, its prompt and line are written into the Terminal Core as bytes, so they become ordinary text (CMD-06).
- **Input Handler.** The Rust half decides what an event means: a shortcut, a command-line key, encoded input or a Vim action. The webview half captures keyboard, input-method and mouse events and normalises them. It never interprets them. Must not draw.
- **Renderer.** The Rust half reads the Terminal Core and the Command Line and decides what to draw, including where each run of text starts and ends, so the ligature rules are tested in Rust (FONT-05 to FONT-08). The webview half paints. Must not change terminal state.

#### Session

`crates/tab/src/session.rs` starts one shell on one pseudo-terminal, through `portable-pty`. The Tab gives it what to start, and a function to call with what the shell writes and with how it ended.

- **What runs** is the Shell setting, the executable and its arguments as written. With the setting not set it is the user's default shell (SW-01):
  - On macOS and Linux that is `$SHELL`. With `$SHELL` not set it is the shell in the user's account entry, else `/bin/sh`. It starts as an interactive login shell, the way `login` starts one: with a `-` before its name, as in `-zsh`, so no argument has to be one that every shell understands.
  - On Windows it is PowerShell 7 if `pwsh.exe` is on the `PATH`, and otherwise the Windows PowerShell that ships with Windows.
  - The Shell setting may name any executable, with any arguments, fish included (FISH-03, FISH-08). Which shells get shell integration is in *Shell integration* below.
- **A shell that cannot start** (FISH-02) is not a session. `Session::start` returns why, as one sentence the Tab shows as it is, and another shell to offer:
  - *Not found:* no file at the path given or, for a name with no folder in it, in no folder of the `PATH`. The sentence then names the folders and asks for the full path. An app opened from the macOS Dock has only `/usr/bin:/bin:/usr/sbin:/sbin`, which is the likeliest reason a shell installed elsewhere is missing.
  - *Cannot run:* a folder, a file that is not executable, or else the OS's own reason.
  - On macOS and Linux a `$SHELL` that cannot run is a shell that cannot start. portable-pty alone would put the account's shell or `/bin/sh` in its place without a word, so a user whose login shell was a fish they have since removed would get `sh` and no explanation. With `$SHELL` not set the default is the account's shell, as before.
  - **The offer.** For the Shell setting, the default shell, or the OS's own shell when the default would not start either. For the default, the OS's own shell: on macOS and Linux the shell in the user's account entry, else `/bin/sh`, as a login shell; on Windows `cmd.exe`. For the OS's own shell, nothing, so the offers end.
  - A pseudo-terminal or a thread that cannot be made is a different error, with nothing to offer: another shell would fare no better.
  - A shell that starts and then exits with an error has started. Its exit is reported like any other (TAB-06, #32).
  - The Tab shows the sentence as a notice over the terminal area (UI-02), with a button that opens the offered shell in the same tab. No other tab is touched. That comes with tabs (#31, #32).
- **The environment** is Twominal's own, plus `TERM=xterm-256color`, `COLORTERM=truecolor`, `TERM_PROGRAM` and `TERM_PROGRAM_VERSION` (SW-02). The name and version are the ones the App Shell has from Tauri, handed down, so the name is still written once.
  - If none of `LC_ALL`, `LC_CTYPE` and `LANG` is set, which is usual for an app opened from the macOS Dock, the shell gets `LANG=C.UTF-8`: UTF-8, and nothing else changed from the defaults. A locale the user did set is kept, whatever it is.
  - On Windows ConPTY already carries UTF-8. PowerShell is told to read and write it by its shell integration script, which is sent even with shell integration off.
- **The directory** is the one the Tab is given, with `PWD` set to it, so a path through a symbolic link stays as it was typed. With none, the shell starts in the home directory and the `PWD` Twominal inherited is removed, so the shell works it out (FISH-01). A directory that has gone by then also means the home directory.
- **Privileges.** The shell is a plain child of Twominal and runs as the user. Nothing in Twominal asks for more (SEC-03).
- **Three threads** per session, so the tab's own thread never waits on the shell:
  - The reader reads up to 64 KB at a time and calls the Tab's function with each piece, untouched. The function may block, and while it does the reader stops reading, which stops the shell (PERF-07).
  - The writer takes what the Tab writes from an unbounded queue. A shell that stops reading, such as `cat` with its output backed up, then blocks only the writer, never the tab.
  - The waiter waits for the shell to end and reports it once, with the exit status or the signal (TAB-06).
- **The exit comes after the last output.** The waiter reports it when the reader has reached the end, or has waited 100 ms with nothing to read. A program the shell left running in the background can keep the pseudo-terminal open, and then the end of the output may not come for as long as it runs. Whatever such a program writes later still arrives after the exit.
- **The size** is the terminal area's rows and columns, given when the session starts and each time it changes (TERM-15). A size of zero is made one.
- **Closing** the session hangs up the shell, as closing a terminal always has. On macOS and Linux that is `SIGHUP`, which the shell passes on to its jobs. Once the waiter has the shell's exit status the shell is not signalled again, because the OS may have given its process ID to another process. On Windows the shell is ended.

#### Shell integration

`crates/tab/src/session/integration.rs` decides which shells get shell integration and how each is given its script (FISH-04, ARC-05). The scripts are in `crates/tab/src/session/integration/` and are built into the executable.

- **Which shells.** The shell is told by the file name of the program that runs, in any letter case and with or without `.exe`: `zsh`, `bash`, `pwsh` and `powershell`. Every other shell, sh and fish included, runs as it is, in passthrough (FISH-03, FISH-08).
- **Which arguments.** A Shell setting with arguments gets integration only when they cannot change how the shell starts up: `-l`, `--login` and `-i` for zsh and bash, and `-NoLogo`, `-NoProfile`, `-Login`, `-l` and `-Interactive` for PowerShell. With any other argument, such as a command to run, the shell runs as written, without integration.
- **No file of the user's is touched.** Each shell is told at startup where Twominal's script is, and the user's own files are read as usual, by the shell or by the script:

| Shell | How it is started | How the user's files are read |
|---|---|---|
| zsh | `ZDOTDIR` names Twominal's folder, whose `.zshenv` is the script. The user's own `ZDOTDIR`, if any, goes in `twominal_zdotdir`. The arguments, and the `-zsh` of a login shell, stay as they were | The script first gives `ZDOTDIR` back, then reads the user's `.zshenv`. zsh then reads `.zprofile`, `.zshrc` and `.zlogin` from the user's `ZDOTDIR`, as always |
| bash | `bash --rcfile <script>`, with `twominal_login=1` when a login shell was asked for: the default shell, or `-l` or `--login` | bash ignores `--rcfile` in a login shell, so the script reads the login files, in bash's own order: `/etc/profile`, then the first of `~/.bash_profile`, `~/.bash_login` and `~/.profile`. Otherwise `~/.bashrc` |
| PowerShell | `-NoExit -EncodedCommand <script>`: UTF-16LE in base64, so it needs no file and no quoting | PowerShell reads the profiles itself, before the script runs |

- **bash is not quite a login shell.** macOS ships bash 3.2, and it gives no way to add a script to a login shell: it ignores `--rcfile` there, and ignores `ENV` under `--posix`. So bash starts as an interactive shell that reads the login files itself. The user's files load as for a login shell, but `shopt login_shell` is off, `$0` is `bash` and `~/.bash_logout` is not read. A bash built to read a system `bashrc` before `--rcfile`, as on Debian and Ubuntu, may read it twice, because `/etc/profile` reads it too.
- **The files.** zsh and bash need the script as a file: zsh finds its startup files only in a folder, and portable-pty cannot hand bash a pipe to read instead. The Session writes them into the folder `Start` gives it, the one only the user can open where the launcher's socket is (§7).
  - They go in a folder of their own, named after what they hold. A file once written never changes, and two versions of Twominal never read each other's.
  - Each is written beside its place and renamed into it, and only when it is missing or different, so a shell starting at the same moment never reads half a file.
  - With no folder, or when writing fails, zsh and bash start without integration.
- **Nothing is left behind.** The variables Twominal passes, `twominal_nonce`, `twominal_zdotdir` and `twominal_login`, are lowercase because they are identifiers (GEN-02). Each script reads them and removes them before the user's files run, so no command and no shell started from this one sees them.
- **The reports.** Before each prompt, the script reports what changed since the last prompt, then that the shell is ready for a command (FISH-05). It runs from `precmd` in zsh, from `PROMPT_COMMAND` in bash and from a wrapped `prompt` function in PowerShell. §7 has the format.
  - The first prompt reports everything, so the first `ready` is the first prompt (SPL-04).
  - A continuation line (`PS2`) reports nothing, so the tab stays in passthrough while the shell asks for more (CMD-02).
  - `$?` is kept, and the user's own prompt and hooks run as before. In zsh the hook moves itself to the end of `precmd_functions` at each prompt, so from the second prompt on `ready` comes after whatever other hooks write.
  - PowerShell lists its cmdlets once, when PSReadLine says it is first idle, because listing them takes a tenth of a second or more. Without PSReadLine they are listed at the first prompt.
- **The nonce.** Each session gets 128 random bits, from the standard library's `RandomState`, which the OS seeds. Every report carries them, and `Session::reports` hands them to the Tab for the Terminal Core (#16). Output that does not carry the nonce is not a report. So text that is only printed, such as a file shown with `cat`, a remote shell or a nested one, cannot fake `ready` and take keys away from a running program.
- **Off.** With the Shell integration setting off (FISH-06), or a shell without integration, `Session::reports` is `None`. The features that need it then fall back: the tab stays in passthrough, a new tab starts in the home directory (TAB-03), there are no prompt jumps (VIM-12), and the splash waits for the first output (SPL-04). Each comes with its own issue. A shell with integration may still never report, if the user's configuration undoes it, so these features must also cope with a report that never comes.
- **Versions.** The scripts use nothing newer than bash 3.2 and zsh 5.0, and PowerShell's runs in Windows PowerShell 5.1 and PowerShell 7. They are tested with bash 3.2.57, zsh 5.9 and PowerShell 7.6 on macOS. Windows PowerShell 5.1 waits for the first Windows build.
- **Not yet.** With the command-line editor on, shell integration must also hide the shell's own prompt and take the finished line from the editor (CMD-01, CMD-04). That is added to these scripts with the editor.

#### Input Handler

`crates/tab/src/input.rs` decides what an input event means. So far keys, pastes and focus are passthrough: they become the bytes the program in the session expects, by the modes the Terminal Core keeps (TERM-08, TERM-09, TERM-11, HW-01). The mouse and the wheel go to a program that asked for them, as reports or as cursor keys, or are Twominal's own, for selection (TERM-17), links (TERM-20) and the scrollback (#26) to act on (TERM-10, HW-02). Shortcuts (#43), the Command Line's keys and Vim mode (#35 to #37) are routed here as each is built.

- **One way in.** `InputHandler::handle` takes an `InputEvent` and the core's modes, and returns one `Action`: bytes for the Session, that the text being composed changed, a mouse gesture that is Twominal's, or rows to scroll the view by. Nothing waits for what may follow, so Esc is sent alone the moment it comes (TERM-08, VIM-05). The Session's writer flushes each write.
- **The grid.** `InputHandler::resize` takes the grid the mouse falls on: the core's size, and a cell's size in CSS pixels. The Tab gives it the Renderer's `Layout` wherever it resizes the core and the Session, as the terminal area, the scale or the font changes (*Renderer*, #31). Until it has, the mouse and the wheel mean nothing.
- **What arrives.** `InputEvent` is shaped after the DOM's events, so the webview half only copies fields:
  - a key's `key` and `code`, the modifiers held, the locks on, and whether it went down, repeated or came up
  - text an input method committed, and the text it is composing
  - a paste, and focus gained or lost
  - a mouse button going down or up, or the mouse moving: the DOM's `button`, `buttons` and `detail`, the modifiers, and where it is in CSS pixels from the top left of the first cell
  - the wheel turning: its `deltaX`, `deltaY` and `deltaMode`, where the mouse is, and the modifiers

  `input/key.rs` reads what a key means, `input/mouse.rs` what the mouse and the wheel mean, and `input/encode.rs` makes the bytes. Any string or number may arrive. A string that names no key sends nothing, a number that is not finite counts as 0, and no number makes more than a screen of output.
- **Layouts (HW-01).** A key sends what the layout typed, which the DOM's `key` gives.
  - On macOS, Option types the character the layout gives it, as in Terminal.app and kitty, so every layout reaches its @, [ and |. On a key that types nothing, such as an arrow, Enter or Backspace, it is Alt, which is what the command line's keys need (Appendix A). With Ctrl or Cmd held, Option is Alt too. There is no setting.
  - AltGr, on Linux and Windows, types the character too. Windows reports it as Ctrl and Alt as well, and those are dropped.
  - With Ctrl or Alt, a key that types no ASCII on its layout is the key at its place on a US keyboard, as in kitty, so Ctrl+C and Alt+B work on Cyrillic and Greek. The DOM's `code` names that place.
  - A dead key sends nothing. What it composes arrives as committed text.
- **Without the kitty keyboard flags, as xterm** sends keys for `TERM=xterm-256color`:
  - Ctrl makes what Xlib's `XkbToControl` makes, which xterm uses, and Ctrl+Shift with a letter is Ctrl with the letter. Alt puts ESC before the key.
  - Enter is CR, or CR LF under LNM. Backspace is DEL, and BS with Ctrl. Shift+Tab is `CSI Z`.
  - The arrows, Home, End and the keypad's Begin follow DECCKM, and with modifiers are `CSI 1;m`, Cmd counting as xterm's Meta, 8. Insert, Delete, the page keys and F5 to F12 are `CSI n ~`. F13 to F35 are what terminfo's `kf13` to `kf35` say: Shift, then Ctrl, with F1 to F12. The menu key is xterm's F16, `CSI 29 ~`.
  - The keypad follows DECKPAM, but for a key that types a character while NumLock is on, as in xterm.
  - A repeat sends what a press sends, and a release nothing.
- **With the kitty keyboard flags (TERM-09)**, as kitty's `key_encoding.c` sends keys, kitty being the protocol's reference.
  - With disambiguation, Esc and every key with Ctrl, Alt or Cmd are escape codes no other key sends, so Ctrl+I is not Tab and Esc does not start an Alt key. The keypad keeps its own codes.
  - The other flags add repeats and releases, the key's shifted form and its US key, every key as an escape code, modifiers alone included, and the text a key types.
  - Enter, Tab and Backspace send what they always have until every key is asked for, so `reset` can still be typed after a program that set the flags crashed.
- **Where it differs.**
  - From xterm: Cmd with a key that types, or with Enter, Tab, Backspace or Esc, sends nothing, since xterm has no encoding for it. Terminal.app and iTerm2 send nothing either. With the kitty protocol it is reported. The US key with Ctrl and Alt on other layouts is kitty's.
  - From kitty with no flag set: Ctrl+Shift with a letter is Ctrl with the letter, and keys xterm has no sequence for, such as the media keys and PrintScreen, send nothing. kitty sends `CSI u` for both, which its specification lets a terminal leave out. The keypad follows DECKPAM, which kitty ignores.
  - The kitty protocol's code for a key is what it types without Shift, and a webview cannot read the layout. A cased character's lowercase is right on every layout. For a shifted symbol, the code is the US key when the layout types the same there, and otherwise the shifted symbol (§9).
- **Pastes (TERM-11).** Under 2004 a paste is bracketed.
  - Line breaks become CR, as Enter types. xterm turns LF into CR too, but CR LF into two.
  - Every other control character but tab becomes a space, as xterm's `disallowedPasteControls` does by default for ESC, BS, DEL, NUL, ENQ, EOT and the stty characters. Twominal cannot read stty, so it takes them all, C1 included. So a pasted ESC cannot end the bracket early, and a pasted Ctrl+C cannot interrupt.
  - The multi-line warning (SEC-06) and the clipboard are #28's.
- **Focus (TERM-11).** `CSI I` and `CSI O`, under 1004.
- **Input methods (HW-01).** The text being composed is kept here, for the Renderer to show at the cursor (*Renderer*). Committed text is sent as it is, in every mode, as kitty sends it.
- **The mouse (TERM-10, HW-02).** A gesture lasts from a button going down until none is held, and is the program's or Twominal's whole, by its first press.
  - It is the program's when the program asked for the mouse and neither Shift nor Meta is held. Shift lets the user select while a program has the mouse (Appendix A), and letting it go in the middle of the drag still finishes the selection. Meta is Cmd on macOS, which opens links (TERM-20), and xterm's reports cannot carry it. Ctrl can be reported, so on Linux and Windows a link under a program that has the mouse takes Shift+Ctrl+click, as in WezTerm.
  - Reports are xterm's, from `button.c`. Mode 9 reports presses of the first three buttons, with no modifiers. 1000 reports presses and releases, 1002 moves while a button is held too, and 1003 every move. A move is reported only in another cell than the last report, with the lowest-numbered button held, or 3 for none.
  - The buttons are 0 to 2, and back and forward 128 and 129. Alt adds 8, Option included, as on keys that type nothing, and Ctrl 16. A move adds 32. A release is 3, but in SGR, which names the button and ends in `m`.
  - Positions count from 1 and are clamped to the grid, as xterm clamps them. The legacy format writes each as a byte, up to 223, and past it writes 0, as xterm does. 1005 writes those past 95 as two bytes of UTF-8, the button's code too, up to 2015. SGR (1006) and urxvt (1015) write numbers, and do not stop.
  - When an event shows a button of the gesture is no longer held, or presses it again, its release was lost, as when the button came up outside the window. The release is then the event's action: the program hears it first, in the same write as what the event sends it, or Twominal's gesture ends where the mouse is. Anything else the event means is dropped, as if it had not come.
  - A gesture that is Twominal's, and a move with no button that the program did not ask for, is a `Pointer`: a press, a drag, a release or a hover, the button, the cell and which half of it, the click count of the press, and the modifiers. The cell is not clamped, so a selection can scroll on past the edge. Selection (TERM-17), links and their targets (TERM-20), the context menu (UI-04) and the primary selection (SW-05) act on it as they are built. A move that stays in the same half of a cell, with the same modifiers, is no action.
- **The wheel (TERM-10, HW-02).** A turn becomes rows: pixels by the cell's height, or columns by its width, lines as they are, pages by the screen. Only the axis it turned most counts, by the deltas, so a trackpad drifting sideways does not report. With Shift and no turn down, the turn sideways is rows, since macOS and Chromium turn a wheel sideways under Shift.
  - A program that asked for the mouse gets at most one report an event, once a row has turned, as xterm.js sends them: 64 up and 65 down, 66 and 67 sideways. A notch is one step however many pixels the OS makes of it, and a trackpad one a row, and the program scrolls by its own step. What turned is kept only to a part of a row, so a flick is not owed afterwards, and starts again from nothing when the wheel turns back or goes elsewhere. Mode 9 reports no wheel, and the wheel does nothing, as in xterm.
  - On the alternate screen a program that did not ask for the mouse gets a cursor key a row, as the key sends it, its release too when the kitty protocol asks, and no more than a screen of them an event (1007). So less and man scroll.
  - Otherwise, and with Shift or Meta, the view scrolls by the rows, fractional so that a trackpad moves it pixel by pixel (#26, ANIM-06).
- **Where the mouse differs from xterm.** 1007 is on until a program turns it off, as in Terminal.app, iTerm2, kitty and Alacritty. Meta keeps the mouse from the program. A gesture stays with whoever had its press, where xterm looks at Shift on each event. A release that was lost is reported. A wheel event is one report however far it turned. 1016's positions in pixels are not reported (*The mouse modes*, Tier 4).
- **The webview half** is written by #31, which first gives a window a tab to type into. It keeps to this, and decides nothing: which key it was, what it typed and which modifiers count are `key.rs`'s.
  - A hidden `<textarea>` in the terminal area has the focus, so the OS sends it keys and input-method text. It follows the frame's caret, the cell where the text being composed starts, so an input method's candidate window opens there (*Renderer*).
  - `keydown` and `keyup` become key events, but not while `isComposing` is set, nor for keyCode 229 or the key `Dead`: those belong to the input method. A `keydown` that is sent is `preventDefault`ed, so no `input` event types it again.
  - `compositionupdate` gives the text being composed, and `compositionend` gives an empty one, then the committed text.
  - An `input` event that no sent `keydown` caused, as from the emoji picker, a press-and-hold accent or dictation, is committed text.
  - `paste` is a paste. On macOS, Edit ▸ Paste fires it.
  - The window gaining and losing focus is focus.
  - `mousedown`, `mouseup` and `mousemove` over the terminal area are mouse events. Mouse events, not pointer events, have the click count and a second button's press. The position is `clientX` and `clientY` less the grid's `getBoundingClientRect()`, not `offsetX` and `offsetY`, since the textarea over the cursor's cell may be the target. `pointerdown` captures the pointer, so a drag is followed outside the window.
  - A `mousedown` is `preventDefault`ed, so the textarea keeps the focus, nothing is selected by the webview, and the middle button neither scrolls nor pastes into the textarea. So are `contextmenu`, `dragstart`, and `auxclick` and the `mouseup` of back and forward, which would leave the page.
  - `wheel` is a wheel event. It is listened to with `passive: false` and `preventDefault`ed, so nothing scrolls elastically or zooms.
  - Moves may be cut to the last of each frame, sent before any other event. Wheel events never are: each may be a step.

#### Renderer

`crates/tab/src/render.rs` decides what the terminal area shows, and `src/tab/renderer.js` paints it on a canvas. The Rust half reads the Terminal Core and is handed the text being composed and the palette. What it returns is a `Frame`: the rows that changed, as text and colors placed on the grid. The webview half measures the font and the terminal area, and paints frames. It decides nothing a test in Rust could check.

- **Four calls.** The Tab makes them all (#31).
  - `Renderer::measure` takes what the webview measured and returns the `Layout`: the grid's size in cells, and a cell's size in device pixels and in CSS pixels. The Tab resizes the core, the Session and the Input Handler to it.
  - `Renderer::frame` takes the core and the text being composed, and returns a frame, or nothing when nothing changed. It only reads the core (§6.1).
  - `Renderer::set_palette` takes the palette the Theme Manager announces (#38). The palette is its: the tab tier has none of its own (ARC-05).
  - `Renderer::redraw` makes the next frame have every row, for a window that has just attached to the tab or switched to it (TAB-09).
- **Device pixels (HW-03).** The canvas has a pixel for each of the display's and is drawn with no scaling, so nothing is resampled.
  - The webview measures the font at its size in device pixels, `devicePixelRatio` times its size in CSS pixels: how far a character advances, and the font's ascent and descent. It measures the terminal area in device pixels too: from `devicePixelContentBoxSize` where the webview has it, which is WebView2, and elsewhere as its size in CSS pixels times the scale, which is exact where the scale is a whole number (§9). A `devicePixelContentBoxSize` more than a pixel from that product is not believed: headless Chromium gives the size in CSS pixels when its scale is only pretended, as in tests.
  - A cell is a whole number of device pixels: the advance rounded, by the ascent and the descent each rounded up. The baseline is the ascent, rounded up, from the cell's top. So every character, line and block starts on a pixel, and every cell is as wide as the next.
  - The grid is as many whole cells as the area holds, and at least one each way. The canvas covers the whole area with the grid at its top left, and what is left at the right and the bottom is the background.
  - A measurement that is not a number above zero is no layout, and the grid stays as it was, so a terminal area that is hidden does not shrink the shell to one cell. The scale is taken between 0.5 and 8, and the area up to 16384 pixels a side, the most a canvas has.
- **A move to another display (HW-03).** When `devicePixelRatio` changes the webview measures again, as `matchMedia` tells it: WebKit's `ResizeObserver` says nothing when only the scale changed. Until the frame laid out for the new scale comes, the browser shows the old pixels scaled, soft for a frame and never blank. Only a frame with every row changes the canvas, its size and its size in CSS pixels, in the same task that paints it, so no frame is painted on a canvas of another layout.
- **Colors.** 0 to 15 are the palette's, 16 to 231 xterm's 6×6×6 cube, whose levels are 0, 95, 135, 175, 215 and 255, and 232 to 255 its grays, from 8 to 238 in steps of 10. An RGB color is drawn as it is.
  - Inverse swaps the text's and the background's colors once the defaults are filled in. Dim text is drawn halfway from its color to its background's. Hidden text takes its cells and draws nothing in them but its background.
  - Bold and italic text are drawn in the font's own bold and italic faces (FONT-09): the font is named with `bold` and `italic`, and the webview picks the faces it has. A font without one has it made from the regular face, as in every browser (§9).
  - Bold never brightens a color, as in kitty, Alacritty and Ghostty, so the palettes keep the contrast THM-05 asks of them. xterm and WezTerm brighten 0 to 7.
  - An underline with no color of its own takes the text's, after inverse.
- **What changed.** For each frame the Rust half builds every row as it is to be drawn: the core's row with the text being composed laid over it, and every color filled in. It sends the rows that differ from the ones it sent last. A new palette, a new layout or font, and a new size send every row, and so does the first frame. The overlays to come, such as the cursor (#25), the selection (#28) and find (#29), are laid over the rows the same way. The core is never asked what changed, so it keeps nothing for the Renderer.
- **Each character at its own cell.** The webview draws each cell's text with `fillText`, at the cell's left edge and on the baseline, both whole pixels. So a webview whose text engine advances a character by another amount cannot move a column (§9).
  - A row is sent as runs: neighbouring cells with the same text color and attributes. In a run each cell holds one character one cell wide, so the webview steps a cell for each character and needs no widths of its own. A wide character, and a character with marks joined to it, is a run of its own with the cells it takes. Blanks with no underline or crossing out are left off both ends of a run.
  - Backgrounds are sent apart, as spans of cells of one color. The palette's background is not sent: each row is painted on it.
  - A sequence the core keeps in several cells is drawn as one, from its first cell over the cells of its parts, when they have the same colors and attributes: two regional indicators, which make a flag; a character and the skin tone in the cells after it; and a character whose marks end in ZWJ, with whatever follows. That is the sequence the core keeps ZWJ for (*Characters and their widths*, Tier 4).
- **Lines, blocks and powerline symbols (FONT-10)** are drawn as shapes, not from the font, so they join across cells whatever the font and the size of a cell. They are box drawing (U+2500 to U+257F), the block elements (U+2580 to U+259F) and powerline's separators (U+E0B0 to U+E0BF). Each is rectangles and polygons in device pixels, inside its cell, filled in the text's color.
  - A line is light, heavy or double. Light is `t` pixels thick: 7% of the font's size, rounded, at least one, and no more than a third of the cell. Heavy is `t + 2·max(1, ⌊t/2⌋)`, which leaves the same room on each side of the light line. Double is two light lines with a light line's room between them.
  - Every line across a cell has the same centre: the light line starts at `⌊(cell − t) / 2⌋`, and the heavy line and the double one are laid around it. A line leaves the cell's edge where its neighbour's enters, so any two characters meet.
  - Each line runs in from its edge. Where lines meet, a single line reaches the far side of the widest line across it, so a corner is square, and stops at a double line's near rail, or crosses both. A double line's rails each turn the corner of the quadrant they border, run on through the centre, or stop at the rail of the double line beside them, so ╔, ╦ and ╬ have their inner and outer corners.
  - The dashed lines leave half a gap at each end, so a row of them is evenly dashed. The rounded corners are quarter ellipses whose ends lie on the light line. The diagonals run corner to corner.
  - A block's eighths are rounded from both of its edges, so ▀ and ▄, or ▉ and ▕, fill a cell between them without a gap or an overlap. The shades are the text's color at a quarter, a half and three quarters over the background.
  - The powerline separators are triangles, half ellipses and lines through the cell's corners and the middles of its edges, so a prompt's segments meet the cells beside them. The other symbols of powerline fonts, such as the branch, are the font's.
- **Underlines and crossing out (TERM-04, TERM-05)** are shapes too, made for one cell when the cell's size changes: single, double, curly, dotted and dashed. A curly underline is one wave a cell, a dotted one a whole number of dots with at least a dot's room between them, and a dashed one a dash as long as the gap after it, so each runs on evenly from cell to cell.
- **The text being composed (HW-01)** is drawn over the cursor's row from the cursor, in the default colors and underlined, each character in the cells `terminal_core::width` gives it. Text that would pass the last column starts further left, and text longer than the row shows its end. A wide character it covers half of is blanked. The frame's caret is the cell where it starts, or the cursor's cell while nothing is composed, and the Input Handler's textarea follows it (#31). Drawing the cursor is #25's.
- **The frame** is JSON text, written by the Rust half and read by the webview half and by no other code. The App Shell passes it on as it is (#31).
  - A frame with every row also has the layout: the canvas's size, the scale, the cell, the baseline, the font and its size, the background, and the shapes of the underlines and of crossing out.
  - Every frame has the colors and the styles its rows use, each once, the shapes of the characters it draws as shapes, the rows that changed, and the caret.
  - It is small. On a screen 200 by 60, a key typed at a prompt makes a frame of about 200 bytes, and a full frame is about 13 KB of plain text, or 30 KB colored as an editor colors code. A program that gives every cell a 24-bit color of its own makes one of about 400 KB, which goes to the webview in a second step (§9).
  - Tauri 2.12 hands a channel message of under 8 KB to the webview as a script (`ipc/channel.rs`), so what a program writes is part of that script (REL-02). Every string is escaped: quotes, backslashes, every control character, and U+2028 and U+2029. Every number is finite. A test escapes every character there is, and reads it back.
- **Painting.** A row is painted in a band of its own, clipped to it: the background, the background spans, the text and the shapes, then underlines and crossing out. So a glyph that reaches past its row, as some emoji do, is cut at the row's edge, and a row painted alone comes out as it does among all the others. `font` and `fillStyle` are set only when they change, and the canvas has no alpha.
- **No animation (ANIM-05).** A frame is painted the moment it arrives, and nothing in the Renderer fades or moves: only the Animator may start an animation, which the architecture test checks (§10). The webview reports a frame drawn at the display's next frame, from `requestAnimationFrame`, so frames never come faster than the display shows them (PERF-07). The report is made even when painting fails, so no frame can stop a tab.
- **The font** is what the App Shell gives the page: a CSS family list and a size in CSS pixels (*Fonts*, Tier 1). The webview half waits for the four faces to load before it measures, and measures again whenever a face loads later.
- **Fallback glyphs fit their cells (FONT-11).** A character the font lacks comes from a fallback font, whose glyph may be wider than its cells. Such a glyph is drawn scaled down, as much across as down, to the width of its cells, on the baseline.
  - A glyph is wider when its advance passes its cells by more than half a pixel a cell, a cell's width being the larger of the cell's and that face's own `0`'s. So no glyph of the font itself is scaled, not even one of a bold face the webview made up.
  - The webview measures each glyph once for each font, and forgets the measures with every frame of every row and whenever a face loads.
  - A glyph taller than its row is cut at the row's edge, as every glyph is.

### Tier 4: Terminal Core

A pure `no_std` crate. Bytes go in through one entry point. Out come the screen state, replies to terminal queries, and what shell integration reports. It holds the screen, scrollback, selection, Vim cursor and find matches. Must not depend on the interface, rendering or OS processes.

`crates/terminal-core/src/` interprets output as xterm does (TERM-01, TERM-06).

- **The parts.** `vte` splits the bytes into characters and control functions. `control.rs` says what each one does, `terminal.rs` does it to the cursor and the modes, and `screen.rs` to the rows. `width.rs` says how many cells a character takes, and `style.rs` what SGR does to the style of the cells. `keyboard.rs` keeps the kitty keyboard protocol's flags. `report.rs` reads shell integration's reports.
- **One way in.** `TerminalCore::interpret` takes bytes and returns the replies and reports they gave, in order, one reply for each query. The Tab writes the replies to the Session (#31). A sequence cut in two by the end of one call is finished by the next, so the screen comes out the same however the output was cut (ARC-03). A character cut in two is held back by the core until the rest comes, because vte 0.15 loses the byte after such a character when another character follows it. `screen()`, `cursor()` and `modes()` say what there is to show, and `width` how many cells a character takes, for the Renderer to lay out the text being composed.
- **The state, as in xterm.**
  - Two screens, primary and alternate, each made of rows that remember whether their line wrapped.
  - One cursor, which stays where it is when the screen switches. It has xterm's pending wrap: a character printed in the last column waits for the next one before the line wraps, which fish relies on. As in xterm, the wrap is pending whether autowrap is on or not, so a character printed after autowrap is turned back on wraps.
  - One saved cursor for each screen.
  - The scrolling region, the tab stops, the character sets G0 to G3, and the modes. DEC Special Graphics is there for the boxes ncurses draws.
- **What it does.**
  - The C0 controls. CAN cancels a sequence, and SUB cancels one and shows `␦`, as xterm does for a VT220.
  - IND, NEL, RI, HTS, DECSC, DECRC, RIS, DECALN, the keypad modes and the designation of character sets.
  - Moving the cursor, erasing, inserting and deleting characters and lines, scrolling, REP, the tab stops and the scrolling region.
  - The modes IRM and LNM, and the DEC private modes 1, 6, 7, 25, 47, 1047, 1048, 1049, 66, 1004, 2004, 9, 1000, 1002, 1003, 1005, 1006, 1015 and 1007. The Input Handler acts on the keyboard, paste, focus and mouse modes.
  - DECSTR, which turns autowrap back on, as in xterm. `tput reset` sends it. It forgets the saved cursor of the screen being shown, and the other screen keeps its own.
  - DECALN, which also makes the region the whole screen and turns origin mode off, as in xterm.
- **Characters and their widths (TERM-02).** Text is UTF-8. A character takes the cells libc's `wcwidth` gives it, as in xterm, which takes them from there or from its own `mk_wcwidth`. Programs count with the same widths, so the grid stays aligned with what they print. Characters are counted one by one, never clustered.
  - The widths are UAX #11's, from `unicode-width`: a wide or fullwidth character takes two cells, and an ambiguous one a single cell, as xterm counts them by default. Where `unicode-width` differs from `mk_wcwidth`, `width.rs` follows `mk_wcwidth`: the soft hyphen takes a cell, and so do the spacing vowel signs of Tamil, Bengali and other scripts. That is about a hundred characters, found by comparing the two over every character.
  - A wide character takes two cells, the second holding nothing of its own. One printed in the last column goes to the next row, as a wrap does, and leaves that column as it was. Without autowrap, or on a screen one column wide, it is dropped. One that fills the last two columns leaves a pending wrap, as a narrow one does in the last.
  - A zero-width character joins the character written last, as in xterm, until something comes between that xterm resets the wrap for: a move of the cursor other than a tab, an erase, an insertion or a deletion, or a switch to or from a screen that is cleared. After that it joins the cell under the cursor. So a combining mark joins its base, and a wrap pending after the base stays pending. A cell keeps two of them, as xterm does by default, and drops the rest. REP repeats the character without them.
  - Emoji are counted the same way, as xterm and `wcwidth` count them. A ZWJ sequence takes the cells of its parts, a skin tone takes two cells of its own, a flag takes two, and VS16 widens nothing.
  - The format characters xterm discards are discarded: the zero width space, the direction marks, embeddings, overrides and isolates, the word joiner, the invisible operators, the deprecated format characters, the byte order mark and the interlinear annotations. ZWJ and ZWNJ, which xterm discards too, and VS15 and VS16, which it drops, are kept with the character before them, so that copying gives back the text (TERM-17) and the Renderer can draw the sequence.
  - Nothing leaves half a wide character. Writing over one of its cells, erasing or deleting one of them, inserting between them or pushing the character to the end of the row, and a narrower screen, blank the whole character, as in xterm.
  - Where it differs from xterm. xterm replaces a character and a mark with the one character Unicode has for both; here they stay as they came. xterm prints a mark on its own in a cell that was never written to, and moves on; here every cell holds at least a blank, so the mark joins it and the cursor stays, as `wcwidth` expects. Given several wide characters in one read with autowrap off, xterm puts the last of them in the last two columns; here each is taken alone, as xterm takes them when they come in separate reads, so the screen does not depend on how the output was read (ARC-03).
- **The kitty keyboard protocol (TERM-09).** `keyboard.rs` keeps the flags a program asked keys to be sent with, and `Modes::keyboard` gives the ones in use, for the Input Handler. kitty defines the protocol, so it is the model, as xterm is elsewhere.
  - Each screen has a stack of its own, of eight flags as kitty's, kept while the other screen is shown.
  - `CSI > flags u` pushes flags, and on a full stack the oldest go. `CSI < n u` pops them, and popping more than there are leaves no flag set. `CSI = flags ; mode u` makes the newest flags these, sets these too or resets these, by its mode, and on an empty stack sets a first entry, as kitty does, which fish relies on: it only ever sets.
  - Seven bits of flags are kept, as in kitty, so a program reads back flags the protocol does not define yet.
  - RIS and DECSTR forget the flags of both screens, as kitty does.
  - vte reads an empty parameter as 0, so `CSI < 0 u` pops one, where kitty pops none.
- **The mouse modes (TERM-10).** `Modes::mouse` says which events a program asked to be told of, `Modes::mouse_format` how they are written, and `Modes::alternate_scroll` what the wheel does on the alternate screen, for the Input Handler.
  - 9, 1000, 1002 and 1003 are one setting, as in xterm: setting one replaces the others, and resetting any turns reporting off.
  - 1005, 1006 and 1015 exclude each other, as xterm's `extend_coords`: setting one makes it the format in use, and resetting one that is not in use does nothing. With none, reports are the legacy bytes. Turning reporting off keeps the format.
  - 1007 is on until a program turns it off, where xterm's is off until a program turns it on.
  - RIS gives them back their defaults, and DECSTR keeps them, as in xterm.
  - 1016, SGR's reports in pixels, is not known. Twominal reports no pixel sizes, so a program could not tell cells by them, and DECRQM's 0 tells it to choose another. Nor are 1001, xterm's highlight tracking, which waits on the program at each press, and the DEC locator.
- **Colors and attributes (TERM-03 to TERM-05).** SGR sets the style that characters are printed in, and each cell keeps the style it was written in. `style.rs` reads SGR as xterm's `CASE_SGR` and `parse_extended_colors` do, but for the underline's styles and color.
  - A color is the default, one of the 256 of the palette, or 24-bit RGB. The core keeps the palette's number, not what it looks like, so a theme change recolors what is on the screen (THM-02, THM-03). The Renderer gives 0 to 15 the theme's 16 ANSI colors (THM-04), and 16 to 255 xterm's 6×6×6 cube and 24 grays (*Renderer*, Tier 3). Bold stays apart from the color: the core never brightens a bold one, and how bold text looks is the Renderer's (FONT-09).
  - 30 to 37 and 90 to 97 are the palette's 0 to 15 for the text, 40 to 47 and 100 to 107 for the background, and 39 and 49 the defaults. 38 and 48 take any color, written in one of xterm's three ways: as the parameters after them (`38;5;n`, `38;2;r;g;b`), as the sub-parameters of the parameter after them (`38;5:n`, `38;2:r:g:b`), or as their own sub-parameters (`38:5:n`, `38:2:r:g:b`). In sub-parameters, an RGB color with four numbers after the `2` starts with the color space, which is skipped (`38:2::r:g:b`). A color with a number missing, or an index or component of 256 or more, is ignored, and the parameters it takes are used up. A kind of color other than 2 and 5 uses up only itself.
  - 1, 2, 3, 4, 7, 8 and 9 turn on bold, dim, italic, underlined, inverse, hidden and crossed-out text. 22, 23, 24, 27, 28 and 29 turn them off, 22 both bold and dim. 21 is a double underline. 0, or no parameter, is the default style.
  - The underline's styles and color (TERM-05) are not xterm's, which ignores SGR 4 with a sub-parameter. They are kitty's, as in VTE, WezTerm and foot: `4:0` to `4:5` are none, single, double, curly, dotted and dashed, and 58 and 59 set the underline's color, written as 38 writes it, and give it back the text's. `4;3` is still underlined and italic. An underline has one style at a time, the last one set, where xterm's 4 and 21 are two attributes.
  - Only 4, 38, 48 and 58 take sub-parameters. Any other parameter with sub-parameters is ignored, as in xterm. So is any other number: blink (5, 6 and 25), because output never animates (ANIM-05, ANIM-07), and fonts, overlines and ideogram marks among them.
  - Every blank a control function makes keeps the colors in use but none of the attributes, as xterm's `ClearCells` does: erased and inserted cells, rows that scroll in, the other half of a wide character written over in part, and the alternate screen cleared. `xterm-256color` tells programs the terminal does this (`bce`), so they paint a background by erasing. A new screen, the cells a resize adds, RIS and DECALN's `E`s have the default style.
  - Both cells of a wide character have its style. A zero-width character joins its cell and leaves the style as it is. REP repeats the character in the style it was printed in, since SGR forgets the character to repeat.
  - DECSC saves the style and DECRC gives it back, or the default style with nothing saved. RIS and DECSTR reset it whole: xterm's DECSTR leaves dim, crossed-out and double-underlined text on, where the VT510's resets the whole rendition, as this one does. DECALN keeps the colors and drops the attributes, as xterm does.
  - vte reads an empty parameter as 0, so `38;2;;0;0` is black, where xterm ignores the color.
- **The replies.** They hold fixed text and numbers, never bytes the program wrote (SEC-05).
  - DA1 is `CSI ? 62 ; 22 c`: a VT220 with ANSI color. fish waits for it before its first prompt. DECID (`ESC Z`) gets the same answer.
  - DA2 is `CSI > 1 ; 2500 ; 0 c`. Programs read the version. vim before 9.0.1168, whose own entry for xterm does not say the mouse has SGR reports, asks for them and for drags from 277; later ones ask anyway. vim stops sending underline colors when the version is from 95 to 2499. From 141 it asks for terminal capabilities with XTGETTCAP, which is never answered. Emacs takes 2001 to 4000 for VTE and asks for nothing, and vim takes 4000 to 4009 for kitty.
  - DSR: the status, and where the cursor is, in the ANSI form and the DEC one.
  - DECRQM, for the ANSI and DEC modes above but 1048, which is an action and not a state. Any other mode is reported as not known.
  - The kitty keyboard protocol's flags in use, `CSI ? flags u`, whatever the query's parameters.
- **Never answered (SEC-05).** The window title and icon label reports (`CSI 20 t`, `CSI 21 t`) and every other window operation, the answerback to ENQ, and every device control string, DECRQSS and XTGETTCAP among them.
- **Ignored.** Whatever is not listed here, as ECMA-48 asks (REL-02).
  - Until their issues: the cursor's shape (#25), and titles, OSC 7, color queries, the bell, links and the clipboard (#27).
  - Always: DECCOLM, which would resize the screen and clear it, KAM and SRM, which would lock the keyboard or echo what is typed, the SGR numbers *Colors and attributes* does not list, blink among them, the mouse modes that are not known (*The mouse modes*), and xterm's modifyOtherKeys, since a program that wants its keys told apart has the kitty keyboard protocol. vim asks for modifyOtherKeys, so it gets xterm's keys, as in Terminal.app.
  - The C1 controls and DEL take no cell, whether a C1 control comes as UTF-8 or as a single byte. vte reads escape sequences only in their 7-bit form.
  - A control function with more than 32 parameters is ignored whole, where xterm acts on its first ones and drops the rest.
- **Where the wrap mark differs from xterm.** A row stops wrapping when an erase reaches its last cell, or when it is cleared whole. xterm's EL 0 and ECH clear the mark whatever they erase, and its EL 1 and EL 2 keep it (`util.c`, `ClearRight`, `ClearLeft`, `ClearLine`). Here EL 0 and ECH clear it only when they reach the last cell, and EL 1 and EL 2 clear it when they reach it, EL 2 always. The mark serves only copying and reflow (TERM-16, TERM-17), and a row whose end was erased does not go on in the next.
- **Until their issues.** Rows that scroll off the top are dropped. A resize cuts or pads the rows without reflowing them, and with fewer rows takes rows from the top only as far as needed to keep the cursor on the screen (#26).
- **Bounded.** `vte` has its default features off. Its buffer for an operating system command is then 256 KiB, boxed, and a longer command is cut short. Every count is clamped to the screen, so no sequence does more than a screenful of work for each of its parameters. REP, which xterm carries out in full however large its count, is cut by whole rows to no more than two screenfuls and a row: by then the characters have scrolled out every row they found, even the last columns a wide character skips, and each further row's worth leaves the screen as it was, so the screen comes out as xterm's does. A cell keeps no more than two zero-width characters. A size of zero is made one.

### What §6.1 does not name

- **Find bar, completion list, notices, Vim mode badge.** Their state belongs to the Terminal Core (find), the Command Line (completions) and the Input Handler (mode). The Renderer draws them over the terminal area from the frame (UI-02).
- **Menus and shortcuts.** A native menu item and its shortcut produce the same action. An item that acts on the app or a window, such as New Window or a theme, is carried out by the App Shell. An item that acts on a tab sends an action that the Input Handler defines (#43).

### The webview holds no model state

A webview shows what Rust last sent it and can be rebuilt from Rust at any time. There is one canvas per window, showing the active tab; switching tabs draws the new tab's full frame at once (TAB-09). The only things a webview keeps are those that exist while a gesture lasts: a tab being dragged, text being composed by an input method, a running animation, and, while it draws a scroll, the part of a row the view has moved so far (ANIM-06). How far to scroll comes from Rust: the Input Handler makes rows of the wheel.

## 4. A tab at run time

```
webview ─ InputEvent ─► App Shell ─ Message ─► Tab thread ─ bytes ─► Session ─► shell
                                                 ▲   │
                    PTY reader thread ─ bounded ─┘   └─ Frame, after the last was drawn ─► canvas
```

SRS §6.2, mapped to code:

1. **Input.** The webview half of the Input Handler sends an `InputEvent`. The App Shell forwards it as a `Message` to the tab's thread without waiting. The Rust half decides what it means.
2. **Command line.** While the shell is ready for a command, keys go to the Command Line. Completions and suggestions are computed off the tab's thread and applied when they arrive (CMD-16).
3. **Session.** The Tab writes input to the Session, whose writer thread passes it on, so a shell that stops reading never blocks the tab. A reader thread reads the shell's output and hands it to the tab's thread through a bounded channel: the Session calls a function the Tab hands down, and the Tab sends on its channel from it.
4. **Terminal Core.** The Tab feeds the bytes in, sends any replies back through the Session, and acts on what shell integration reports.
5. **Renderer.** The Tab asks the Renderer for a frame, which holds the rows whose drawing changed, and sends it. It sends the next one only after the webview reports the last was drawn, which the webview does at the display's next frame. A hidden window's frames are paused, so its tabs send none until it shows again.

What this gives:

- **A tab never waits for another tab.** No lock is shared between tabs, and the App Shell only posts messages (REL-01, ARC-04).
- **Heavy output cannot pile up.** The bounded channel stops the reader, which stops the shell, and at most one frame is in flight (PERF-07). Ctrl+C is a message like any other and is handled between two chunks of output (AT-12).
- **A tab needs no window.** It runs from the moment it is created, and a window attaches to it later and gets a full frame. So the first session starts while the splash plays (SPL-03), keys typed during the splash wait in the Input Handler (SPL-05), and background tabs cost no drawing.
- **A panic ends one tab.** Release builds unwind, so a panic on a tab's thread closes that tab with a notice and leaves the rest running. That is why the release profile has no `panic = "abort"`.

Prior art in this repository: `origin/codex:src-tauri/src/terminal/flow.rs` for output back-pressure.

## 5. Cross-cutting concerns

Each concern has one place (ARC-05).

| Concern | Lives in | How others get it |
|---|---|---|
| Settings | Settings Store | The App Shell passes each change on. Tabs get theirs as a message |
| Command history | History Store | Tabs hold it as the trait the Command Line defines |
| Theme resolution | Theme Manager | The App Shell passes the palette to windows and tabs |
| Animation policy | Animator | Views are handed the Animator and ask it. Stylesheets declare no transitions or keyframes |
| Shell-specific behavior | The Session's shell integration | It reports through escape sequences that the Terminal Core announces |
| fish-style command-line features | Command Line | — |
| Talking to Tauri | App Shell | Everything else is handed what it needs |

The cursor blink and smooth scrolling are drawn by the Renderer and follow their own settings. They are not interface animations (ANIM-05 to ANIM-07).

## 6. What 1.0 does not contain

No plugin system, no scripting engine and no network component (ARC-06).

- **Dependencies are closed.** A direct dependency that is not in the table in §2 fails the architecture test.
- **The webview cannot go online.** The content security policy in `tauri.conf.json` allows the app's own files and the IPC scheme, and nothing else.
- **Shipped builds listen on no network port.** The app's files are embedded and served to the webview through Tauri's own scheme. Only `cargo tauri dev` runs a local server, and that server is part of the Tauri CLI, not of Twominal. The one thing Twominal listens on is the launcher's socket (§3): a file in a folder only the user can open, which no other machine can reach.
- **Tauri brings network crates for phones only.** `Cargo.lock` lists `reqwest` and `hyper` because Tauri uses them on Android and iOS. They are not compiled for desktop: `cargo tree -i reqwest` prints nothing.

## 7. Files and formats

| What | Where | Format |
|---|---|---|
| Settings | `config.toml` in the OS configuration folder `twominal/`: `~/Library/Application Support` on macOS, `$XDG_CONFIG_HOME` or `~/.config` on Linux, `%APPDATA%` on Windows | TOML. One file (SET-03) |
| Command history | `history` in the OS data folder `twominal/`: `~/Library/Application Support` on macOS, `$XDG_DATA_HOME` or `~/.local/share` on Linux, `%LOCALAPPDATA%` on Windows | Plain text, one command per line, a line break written as `\n` and a backslash as `\\`. Readable only by the user (SEC-02) |
| Launcher socket and lock | `twominal.sock` and `twominal.lock`, in the user's own temporary folder on macOS and in `$XDG_RUNTIME_DIR` on Linux. A debug build's are `twominal-dev.sock` and `twominal-dev.lock` | One byte for the version of the exchange, then the directory's bytes. Neither file keeps anything |
| Bundled font | `src/fonts/`: `JetBrainsMono-Regular.woff2`, `-Bold`, `-Italic` and `-BoldItalic` from release 2.304, and `OFL.txt`, the font's copyright notice and license (FONT-01). The installers also hold `OFL.txt` as a file of its own, as `fonts/OFL.txt` among their resources | WOFF2, which every webview reads, at a third of the size of the TrueType files. Embedded with the rest of `src/` |
| Logo | `src/logo.svg`. The app icons in `src-tauri/icons/` are generated from it (GEN-03) | SVG, made of shapes only: no text, no style sheet, nothing loaded from elsewhere |
| Shell integration | The scripts are in `crates/tab/src/session/integration/`, built into the executable. zsh's and bash's are written to the launcher's folder, as `twominal-<hash of the scripts>/zsh/.zshenv` and `twominal-<hash of the scripts>/integration.bash`. PowerShell's goes on its command line | One script per shell, given to the shell at startup. No file of the user's is touched (FISH-04) |
| Webview code | `src/` | Served as it is |

Prior art in this repository: `origin/codex:src-tauri/shell-integration/` for integration that leaves the user's files alone.

### Shell integration reports

Each report of shell integration (FISH-05, §3) is one escape sequence:

```
ESC ] 2222 ; <nonce> ; <key> ; <value> BEL
```

- **2222** is Twominal's own OSC number. A terminal that does not know it ignores it.
- **The nonce** is the session's. A sequence with any other nonce is not a report.
- **The value** has every byte outside `!` to `~`, and every `%` and `;`, written as `%XX`, so it is printable ASCII and holds no `;`. A reader decodes every `%XX`.
- **A list** is its names, each encoded, separated by single spaces.

| Key | Value | Sent |
|---|---|---|
| `cwd` | The working directory. zsh's and bash's `$PWD`, kept as typed through symbolic links. PowerShell's `$PWD.ProviderPath`, and only in the file system | When it changed |
| `path` | `PATH`, as the shell has it | When it changed |
| `aliases` | The names of the aliases. zsh's include its global aliases | When they changed |
| `functions` | The names of the functions, but Twominal's own | When they changed |
| `cmdlets` | PowerShell only: the names of the cmdlets, which count as builtins (CMD-07, CMD-09) | Once, when PowerShell is first idle |
| `ready` | None. The shell is ready for a command | Before each prompt, last |

*When it changed* means since it was last sent, so the first prompt sends them all. A list can be long: a zsh with a plugin manager has a thousand functions or more, so a report can be tens of kilobytes. The Terminal Core takes a report that ends in BEL and is shorter than 256 KiB from `2222` to the end of its value. It drops any other whole: one ended otherwise was cut short or is not one of the scripts', and one that long was cut short by the core's buffer.

### The settings file

`config.toml` has one key for each setting of SRS Appendix B, in the same order, except that the abbreviations come last: they are a table, and in TOML a table follows the plain keys. A key that is missing means the default, so an empty file and no file both mean every default. Words may be written in any letter case.

| Setting | Key | Value | Default |
|---|---|---|---|
| Theme | `theme` | `"dark"`, `"light"` or `"system"` | `"system"` |
| Font | `font` | A font name | `"JetBrains Mono"` |
| Font size | `font_size` | A number from 6 to 72 | `13` |
| Font ligatures | `font_ligatures` | `true` or `false` | `true` |
| Vim mode | `vim_mode` | `true` or `false` | `true` |
| Animations | `animations` | `true` or `false` | `true` |
| Splash screen | `splash_screen` | `true` or `false` | `true` |
| Cursor style | `cursor_style` | `"block"`, `"bar"` or `"underline"` | `"block"` |
| Cursor blink | `cursor_blink` | `true` or `false` | `true` |
| Scrollback | `scrollback` | A whole number from 0 to 100000 | `10000` |
| Shell | `shell` | A list of strings: the executable, then its arguments | Not set: the user's default shell (SW-01) |
| Shell integration | `shell_integration` | `true` or `false` | `true` |
| Command-line editor | `command_line_editor` | `true` or `false` | `true` |
| Command history | `command_history` | `true` or `false` | `true` |
| New tab directory | `new_tab_directory` | `"current"` or `"home"` | `"current"` |
| Confirm before closing running programs | `confirm_close` | `true` or `false` | `true` |
| Bell | `bell` | `"visual"`, `"audible"` or `"off"` | `"visual"` |
| Clipboard writes by programs | `clipboard_write` | `"allow"`, `"ask"` or `"deny"` | `"allow"` |
| Clipboard reads by programs | `clipboard_read` | `"allow"`, `"ask"` or `"deny"` | `"deny"` |
| Multi-line paste warning | `paste_warning` | `true` or `false` | `true` |
| Abbreviations | `[abbreviations]` | One `word = "expansion"` line for each. A word has no spaces | None |

Keyboard shortcuts (SET-06), theme color overrides (THM-09) and font features (FONT-12) get their own tables in this file, each added by its feature.

The store checks only what the file can say. Whether the font is installed is the App Shell's to find out (*Fonts*, §3), and whether the shell exists is the Session's (FISH-02).

What happens to a file that is not right (SET-05):

| The file | Settings used | Notice |
|---|---|---|
| Is missing | Every default | None. The file is created the first time a setting is saved |
| Cannot be read, or is not TOML, at startup | Every default | One, naming the file and the problem, with the line for a syntax error |
| Becomes unreadable or stops being TOML while Twominal runs | The ones in use stay. A file caught half-edited should not reset the font and the theme | The same |
| Has a value of the wrong type, out of range or not one of the words | That setting's default. The others apply | One for each, naming the key, the problem and the default used |
| Has a key that is not a setting | — | One, naming the key. The key is kept when the file is saved |

Notices follow the file: when it is fixed and saved, they go.

Saving a setting changes only the keys whose value changed. Comments, the order of keys and keys the store does not know stay as they were. The new text is written to a temporary file beside `config.toml` and renamed over it, so a reader never sees half a file. A `config.toml` that is a symbolic link stays one, and what it points to is rewritten.

## 8. Testing

- **`cargo test --workspace`** runs everything below that exists.
- **Architecture test**, `src-tauri/tests/architecture.rs`. It reads the two marked tables in this document and checks the code against them (§10).
- **Bundled font**, `src-tauri/tests/fonts.rs`. It checks that the four faces are WOFF2 with TrueType outlines, which Tauri serves as fonts, that `src/app.css` names each with its weight and style, and that the license is there and the installers hold it (FONT-01, C-3).
- **Identity test**, `src-tauri/tests/identity.rs`. It checks the product's name, that the code spells it only as `Twominal` or `twominal`, the lowercase identifiers, and that the logo stands alone (GEN-01 to GEN-03).
- **Settings Store.** `src-tauri/src/settings/tests.rs` gives each test a configuration folder of its own, and covers loading, each kind of bad value, hand edits, saving and the wording of the notice for a font that is not installed (SET-02 to SET-05, AT-13). The store does not depend on Tauri, so it needs no window.
- **App Shell.** `src-tauri/src/app/tests.rs` covers what needs no window: window titles and labels, what each menu item does, the shortcut on each OS, the font a page is given and its size on each OS, the font size each zoom item makes, the notice for a font that is not installed, and the launcher's arguments, texts and hand-over, in a folder of the test's own. On macOS it finds Menlo, which every Mac has, and no font of a made-up name. `src-tauri/tests/launcher.rs` runs the executable for what it answers without starting Tauri (GEN-05). The windows and menus themselves, and a window the launcher asks for, are checked by hand (MAINT-02).
- **Session.** `crates/tab/src/session/tests.rs` starts `/bin/sh` on real pseudo-terminals and checks the bytes both ways, the environment, the directory, the size, the exit status, closing, and an exit while a background program holds the terminal (SW-01, SW-02, FISH-01, TERM-15, SEC-03). It also checks a shell that cannot start: a Shell setting that names no file while another session keeps working, a name missing from the `PATH`, a file that is not executable, and a `$SHELL` that has gone (FISH-02, AT-16). These run on macOS and Linux.
  - Shell integration is tested the same way (FISH-04 to FISH-06). zsh, bash and PowerShell start in a home folder of the test's own, with the user's files in it, and the tests check the reports, that the user's files are read in the usual order, and that the home folder is the same afterwards, file for file. Twominal's own folder, the variables it passes and the setting turned off are checked too. A test whose shell is not installed says so and passes. On macOS, `brew install powershell` installs PowerShell 7.
  - The choice of locale and of PowerShell, the waiter's rule for when to report the exit, how a program is looked for and why it cannot run, the offers and their wording, and which shells get shell integration and what it adds to their command are tested on every OS, but no shell has yet been started on Windows.
- **Terminal Core.** Everything runs with no window (ARC-03).
  - **The conformance suite** (MAINT-03), `crates/terminal-core/tests/conformance.rs`, runs the cases in `crates/terminal-core/tests/conformance/`, one file for each area of §3. A case gives a size, the bytes a program writes, and the screen, cursor, modes and replies that must follow, drawn as text. A row of the screen is the text of its cells: a wide character takes two columns of it, as in a monospace font, and a combining mark follows its character. The cells' styles are drawn the same way, a letter for each cell, and the case says which style each letter stands for. Each expectation is what xterm does, taken from its control sequences document and its source, or a deviation this section documents, which the case's comment names. Every reply must be one the case expects, so a query that must not be answered fails if it is (SEC-05). Each case runs twice, as written and a byte at a time (ARC-03). The suite uses only the public interface, as the Tab does, and is where each TERM requirement the Terminal Core meets gets its test (MAINT-02).
  - **Unit tests**, `crates/terminal-core/src/tests.rs`, check what the public interface does not show: internal state, the reports of the shell integration, resizing, the widths, REP against printing the characters out, and that the same bytes give the same state whether they come whole, a byte at a time, cut at any one place or in random pieces (ARC-03). Characters cut in two are checked on their own. An ignored test measures plain text: `cargo test --release -p terminal-core -- --ignored` (PERF-06).
  - **Fuzzing** (REL-02). `crates/terminal-core/fuzz/` holds two cargo-fuzz targets. `interpret` gives hostile output in pieces, with resizes between, and checks the state's rules after each step and that every reply is fixed text and numbers (SEC-05). `split` checks that the output gives the same state and events whole, a byte at a time and in pieces (ARC-03). An input's first bytes choose the size, up to 160 by 60, and the cuts, so libFuzzer explores them too. `terminal.dict` gives it the pieces of sequences. What they check is `crates/terminal-core/src/fuzz.rs`, which `cargo test` also runs, on stable, with hostile output made up from the same pieces. Run each target from `crates/terminal-core` for half an hour after a change to the Terminal Core, and add whatever it finds as a test: `cargo +nightly fuzz run interpret -- -dict=fuzz/terminal.dict -timeout=5 -max_len=16384 -max_total_time=1800 -rss_limit_mb=4096`, and `cargo +nightly fuzz run split -s none -- -dict=fuzz/terminal.dict -timeout=5 -max_len=16384 -max_total_time=1800`. `-timeout` makes a slow input a hang, which REL-02 forbids. `interpret` runs under AddressSanitizer, for the `unsafe` code in vte. Its memory then grows to almost 3 GB in half an hour and levels off there, though the Terminal Core holds a few megabytes of it: AddressSanitizer's allocator keeps what each input's rows freed, and a row of 32-byte cells frees twice what one of 16 did. So it gets 4 GB, where libFuzzer's 2 GB stops it within the half hour. `split` checks logic, not memory, so it runs without: under AddressSanitizer its memory grows by about 100 MB a minute until libFuzzer stops it at 2 GB, though the Terminal Core holds none of it, and without it runs three times as fast.
  - `crates/tab/tests/programs.rs` runs sh, fish, vim, less, `git log`, top, `fish_key_reader` and zsh on real pseudo-terminals through the Session, with the Terminal Core answering their queries. It checks what they show and that the screen is given back when they end (TERM-01), that vim's cursor lands where the core put the characters of a file of wide characters, combining marks and emoji (TERM-02), and that the colors git writes reach the cells through less, as do vim's 24-bit colors, its attributes and the background it paints by erasing (TERM-03, TERM-04). Each runs in a home folder and environment of its own. A program that is not installed is said so, and its test passes.
  - The same file types into them through the Input Handler, as DOM events, by the modes they set. `fish_key_reader` turns the kitty keyboard protocol on and reads Ctrl+I, Tab, Esc and an Alt key apart (TERM-09). fish edits with Ctrl+W through the protocol and turns it off for the commands it runs, so cat reads Ctrl+D as xterm sends it (USE-03). zsh keeps a bracketed paste in its line until Enter (TERM-11). vim moves with the arrows in application mode and leaves insert mode on Esc at once, and hears focus come and go (TERM-08, TERM-11).
  - The mouse and the wheel go through it too (TERM-10). vim asks for SGR reports with drags and keeps its underline colors with DA2's version. A click moves its cursor, a drag selects what `d` deletes, and a Shift+click is not vim's. A notch scrolls it by its own step, as do a trackpad's few pixels once they make a row. On a terminal 300 columns wide a click lands in column 250, which the legacy format cannot write. less scrolls a line a row through the cursor keys of 1007, and with `--mouse` a line a report.
  - The Renderer draws what the core holds (*Renderer*, §3): sh's `printf` gives it the 16, 256 and 24-bit colors, attributes, box drawing, blocks, a powerline separator, a wide character and a combining mark, and the frame has them as runs, spans and shapes, as JSON a reader accepts. vim's 24-bit colors and the background it paints by erasing come out as exact spans, and a key typed at a prompt sends one row in a frame of under 8 KB.
  - `crates/tab/src/session/tests.rs` checks that the Terminal Core reads zsh's reports as the scripts write them, a list of over 60 KB among them.
- **Tab tier.** The `tab` crate does not depend on Tauri, so the Command Line, key handling and the Renderer's choice of text runs are unit-tested without a window.
  - **Input Handler.** `crates/tab/src/input/tests.rs` gives DOM key events, pastes, focus changes and input-method text, and checks the bytes: xterm's encodings of every key and modifier, layouts, macOS Option and AltGr, and the kitty protocol's flags, with kitty's own cases from `kitty_tests/keys.py`. It also checks that, with every key reported, every key and every set of modifiers reaches the program, that without the protocol every key xterm can send does, and that no string the webview sends can make it panic (TERM-08, TERM-09, TERM-11, HW-01).
  - It gives mouse and wheel events too, and checks xterm's reports in each mode and format, the limits of the legacy and UTF-8 formats, the gestures Shift and Cmd keep from the program, lost releases, the cells and halves of Twominal's gestures, how a trackpad's pixels and a notch become steps, the cursor keys of 1007, and that no number the webview sends can make it panic or send more than a screen of keys (TERM-10, HW-02).
  - **Renderer.** `crates/tab/src/render/tests.rs` checks the colors of all 256 indices, inverse, dim and hidden text, and that bold never brightens. It lays out the cell at every scale from 1 to 3 and every font size from 6 to 96 CSS pixels, which is 72 pt on Linux and Windows, and gets no layout from numbers that are no size. It checks that a frame holds exactly the rows that changed: random output, with each frame painted onto a model of the canvas, gives the canvas a full frame gives. It checks the runs, the spans, the sequences drawn as one and the text being composed. The shapes are drawn into pixels in the test at cells of many sizes from 3 by 6 to 40 by 80: each line leaves its edge where its weight says, so any two characters meet, each double-line character has the pieces it should, blocks that make a cell between them tile it, and nothing reaches out of its cell. The escaping of the frame is checked over every character there is, by a reader written in the test (HW-03, FONT-10, TERM-03 to TERM-05).
- **Webview.** It has no logic of its own to unit-test. What it draws is checked by the acceptance scenarios (SRS §7) as documented manual tests (MAINT-02). The Renderer's half is checked by hand on each OS once #31 puts a tab in a window: text, box drawing, blocks and powerline symbols at scales 1 and 2, bold and italic faces, fallback glyphs inside their cells, and a window dragged between displays of different scales (HW-03, FONT-09, FONT-10, FONT-11). So are the font size shortcuts on each OS, and the notice for a font that is not installed (FONT-02, SET-05). That nothing animates is checked by the architecture test (ANIM-05).

## 9. Risks

| Risk | Requirement | Settled by | If it fails |
|---|---|---|---|
| Canvas 2D is slow in WebKitGTK on Linux | PERF-04, PERF-05 | #48 | Draw with a WebGL2 glyph atlas behind the same `Frame` |
| A webview starts too slowly on some OS | PERF-01 | #48 | Nothing inside this design. The target or the framework is reopened |
| The three webviews draw text slightly differently | PORT-02, FONT-05 | #22, by drawing each character at its own cell. #24, for ligatures | A ligature is drawn over exactly its cells, so differences cannot move a column |
| WebKit cannot say how many device pixels the terminal area covers | HW-03 | #22: macOS's scale is 1 or 2, and WebKitGTK is on GTK 3, whose scale is whole, so the size in CSS pixels times the scale is exact | A display with a fractional scale under Wayland is scaled by the compositor, which blurs every GTK 3 app. Twominal moves to WebKitGTK on GTK 4 when Tauri does |
| Drawing each cell with its own `fillText` is too slow | PERF-04, PERF-05 | #48 | The webview half keeps each glyph it draws in a canvas of its own and copies it, behind the same frame |
| A font has no bold or italic face | FONT-09 | Nothing can settle it | The webview makes one from the regular face. The bundled font has all four (#23) |
| The App Shell and a webview find a font by different names | SET-05 | Nothing can settle it | Fonts are found by their family names. A font named by its full or PostScript name, which WebKit also finds, is drawn but gets the notice. The settings screen offers family names only (#42) |
| GTK does not take Ctrl+Shift+= for the Increase Font Size item, since with Shift the key is `plus` | FONT-02 | By hand on Linux | The Input Handler takes the font size keys as shortcuts (#43) |
| A frame under 8 KB reaches the webview as a script | REL-02 | #22: every string is escaped, and a test escapes every character | Frames are sent as bytes, which Tauri passes as numbers |
| IPC adds too much to each key | PERF-04 | #48 | Frames while typing carry only the rows that changed. In Tauri 2.12 a channel message of up to about 8 KB of JSON goes to the webview in one step, and a larger one needs a second |
| Input methods need decisions in the webview | HW-01 | #31, against the contract in §3 | The webview half of the Input Handler may hold a composition, and nothing else |
| A webview cannot read the keyboard layout | TERM-09 | Nothing can settle it in a webview | The kitty protocol's code for a shifted symbol on a layout other than US is the shifted symbol (§3). If programs need more, the App Shell asks the OS for the layout and hands it down |
| A wheel notch is a different number of pixels in each webview | HW-02, TERM-10 | #31, by hand on each OS | A program hears one report an event, so a notch is one step to it anyway. If the view scrolls too far or too little a notch, the webview half sends the DOM's `wheelDeltaY` too, which tells a notch from a trackpad |
| Chromium sends a trackpad's pinch as the wheel with Ctrl | TERM-10 | #31, on Windows | A pinch over a program that has the mouse is reported as Ctrl and the wheel. If that does harm, the webview half drops wheel events with Ctrl that no Ctrl key went down for |
| Mouse moves cost an IPC message each | PERF-04 | #48 | The webview half sends at most one move a frame, and the Input Handler none for a move that stays in its cell |
| A webview does not follow the OS reduced-motion setting | SW-04, ACC-01 | #40 | The App Shell reads the OS setting itself and passes it to the Animator |
| A canvas cannot be read by screen readers | ACC-04 | #46 | The Renderer keeps a hidden text copy of the visible rows for them |
| Another crate turns on vte's `std` feature, which makes its buffer for operating system commands unbounded | ARC-02, REL-02 | The build: `TerminalCore::new` calls `vte::Parser::new_with_size`, which is there only without `std` | Change this document first |
| A report of the shell integration is 256 KiB or longer | FISH-05 | Nothing can settle it | The Terminal Core drops it, and the features that need it fall back as with no report (§3) |
| A program asks for the cursor's position over and over and never reads the replies | REL-01 | #31 | The Session's writer queue has no bound, so the Tab must cap the replies waiting to be written |
| A user's configuration undoes shell integration, so no report comes | FISH-05 | Nothing can settle it | The features that need it fall back, as with the setting off (§3) |
| Windows PowerShell 5.1 runs the integration script differently from PowerShell 7 | FISH-04 | The first Windows build | The script changes. It needs no file, so nothing else does |

Twominal has so far been built and run only on macOS.

## 10. Requirements this document answers

| ID | How it is met | Checked by |
|---|---|---|
| MAINT-01 | §1 gives every component of SRS §6.1 one home | `every_component_has_its_home` |
| ARC-01 | The homes are the components. No code lives outside one | `every_component_has_its_home`, `no_code_outside_a_component` |
| ARC-02 | Three crates: the app for tiers 1 and 2, `tab` for tier 3, `terminal-core` for tier 4. The rule in §1 | Cargo; `components_do_not_name_their_siblings`, `only_the_app_shell_names_tauri`, `terminal_core_is_no_std` |
| ARC-05 | §5 | `animation_policy_lives_in_the_animator`, and by review against §5 for each feature |
| ARC-06 | §6 | `dependencies_match_the_design`, `webview_reaches_nothing_but_ipc` |
| ARC-07 | §1 is the one-page overview, and the test reads it | `every_component_has_its_home` |
| ANIM-05 | Output is painted when it arrives, and only the Animator starts an animation (§3, §5) | `animation_policy_lives_in_the_animator` |

The tests are in `src-tauri/tests/architecture.rs`.

## 11. Changing this document

- Change the document first, then the code. The two marked tables are the test's input, so the order is enforced.
- Adding a top-level component also needs a change to the SRS (ARC-01).
- Raise the version and date at the top with each change.

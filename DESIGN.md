# Twominal — Design

**Version:** 0.1 · **Date:** 2026-10-14 · **Tracks:** [SRS](SRS.md) v0.6

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
| Renderer | Canvas 2D. Each row is drawn as runs of same-style text | FONT-04 to FONT-08, FONT-11, SW-06, HW-03 |
| Pseudo-terminal | `portable-pty`, which uses ConPTY on Windows | SW-01, PORT-01 |
| Rust ⇄ webview | Tauri commands in; events and channels out | PERF-04, PERF-07 |
| Settings file | TOML | SET-03, SET-04 |
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

### Direct dependencies

Every crate Twominal depends on directly is listed here, and nothing else may be (ARC-06). Add the row first, then the dependency.

<!-- dependencies:begin -->
| Crate | Depends on | For |
|---|---|---|
| `twominal` | `tauri` | Windows, menus, the webview and IPC |
| `twominal` | `tauri-build` | The build script `tauri` needs |
| `twominal` | `tab` | The tab tier |
| `tab` | `terminal-core` | The Terminal Core |
<!-- dependencies:end -->

Planned, and added by the issue that first needs each:

| Crate | Will depend on | For | Issue |
|---|---|---|---|
| `terminal-core` | `vte`, with default features off so it stays `no_std` | Parsing escape sequences | #16 |
| `tab` | `portable-pty` | Pseudo-terminals | #13 |
| `twominal` | A TOML library that keeps comments when it saves | The configuration file (SET-04) | #9 |
| `twominal` | A clipboard library that reaches the Linux primary selection | SW-05 | #28 |
| `twominal` | A font library that lists installed monospaced families | FONT-02 | #23 |

## 3. Components

As of this version every Rust home exists as a skeleton: types that say who owns what, and no behaviour. Of the webview homes only `src/app.js` exists. The others are created by their issues, at the paths in §1.

### Tier 1: App Shell

- **Owns** the stores, the Theme Manager, every window, the native menus and the splash screen. Each window has one Tab Manager.
- **Is the only code that names Tauri.** Every command and event is defined in `app.rs`. Commands are named `<component>_<verb>`, such as `app_ready` or `tabs_open`, and events `<component>:<what>`.
- **Wires the stores and managers.** It hears a settings change and passes it to the Theme Manager, the Tab Managers and the webviews. They never name each other.
- **Creates windows in code, not from `tauri.conf.json`.** A window then opens with the resolved background and the settings it needs for its first frame, so there is no flash of the wrong theme (SPL-02) and reduced motion applies from the start (SPL-07). Built in #10.
- **Its page** hosts the tab bar, the canvas, the splash screen and the settings, About and help screens (UI-03).
- **Must not** contain terminal logic.

### Tier 2: stores and managers

- **Settings Store.** Owns the file, its format, every default in Appendix B and every validation rule. Names no other component.
- **History Store.** Owns the shared command history and its file. It implements the history trait that the Command Line defines, and tabs hold it only as that trait (ARC-04). Must not store terminal output.
- **Theme Manager.** Takes the Theme setting and the OS appearance, both passed in by the App Shell, and announces the palette. `src/theme.js` applies the announced palette to the page.
- **Animator.** Lives in the webview, because that is where animations run and where the webview reports the OS reduced-motion setting. Views ask it to animate, and it decides how, or whether. Must not delay input or output.
- **Tab Manager.** One per window. Holds a `tab::Handle` for each tab and nothing else, so it cannot reach into a tab's session or screen. `src/tabs.js` draws the tab bar.

### Tier 3: a tab

- **Tab.** Owns one Session, Terminal Core, Command Line, Input Handler and Renderer (TAB-01), and is the only code that passes things between them. It is created with one bundle, `tab::Host`, holding everything it may ask of the layers above: the history trait, the clipboard, opening links, and where to send frames and notices. The bundle is added with the first feature that needs it.
- **Session.** Starts the shell on a pseudo-terminal and carries bytes. Holds the shell integration for each supported shell. Must not interpret output.
- **Command Line.** A state machine: keys in, an edited line out. Must not take input in passthrough or depend on fish. When a command runs, its prompt and line are written into the Terminal Core as bytes, so they become ordinary text (CMD-06).
- **Input Handler.** The Rust half decides what an event means: a shortcut, a command-line key, encoded input or a Vim action. The webview half captures keyboard, input-method and mouse events and normalises them. It never interprets them. Must not draw.
- **Renderer.** The Rust half reads the Terminal Core and the Command Line and decides what to draw, including where each run of text starts and ends, so the ligature rules are tested in Rust (FONT-05 to FONT-08). The webview half paints. Must not change terminal state.

### Tier 4: Terminal Core

A pure `no_std` crate. Bytes go in through one entry point. Out come the screen state, replies to terminal queries, and what shell integration reports. It holds the screen, scrollback, selection, Vim cursor and find matches. Must not depend on the interface, rendering or OS processes.

### What §6.1 does not name

- **Find bar, completion list, notices, Vim mode badge.** Their state belongs to the Terminal Core (find), the Command Line (completions) and the Input Handler (mode). The Renderer draws them over the terminal area from the frame (UI-02).
- **Menus and shortcuts.** A native menu item and its shortcut produce the same action. The Input Handler defines the actions; the App Shell's menus send them.

### The webview holds no model state

A webview shows what Rust last sent it and can be rebuilt from Rust at any time. There is one canvas per window, showing the active tab; switching tabs draws the new tab's full frame at once (TAB-09). The only things a webview keeps are those that exist while a gesture lasts: a tab being dragged, text being composed by an input method, a running animation, and the part of a row scrolled so far (ANIM-06).

## 4. A tab at run time

```
webview ─ InputEvent ─► App Shell ─ Message ─► Tab thread ─ bytes ─► Session ─► shell
                                                 ▲   │
                    PTY reader thread ─ bounded ─┘   └─ Frame, after the last was drawn ─► canvas
```

SRS §6.2, mapped to code:

1. **Input.** The webview half of the Input Handler sends an `InputEvent`. The App Shell forwards it as a `Message` to the tab's thread without waiting. The Rust half decides what it means.
2. **Command line.** While the shell is ready for a command, keys go to the Command Line. Completions and suggestions are computed off the tab's thread and applied when they arrive (CMD-16).
3. **Session.** The Tab writes input to the Session. A reader thread reads the shell's output and hands it to the tab's thread through a bounded channel.
4. **Terminal Core.** The Tab feeds the bytes in, sends any replies back through the Session, and acts on what shell integration reports.
5. **Renderer.** The Tab builds a frame from what changed and sends it. It sends the next one only after the webview reports the last was drawn.

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
- **Shipped builds listen on nothing.** The app's files are embedded and served to the webview through Tauri's own scheme. Only `cargo tauri dev` runs a local server, and that server is part of the Tauri CLI, not of Twominal.
- **Tauri brings network crates for phones only.** `Cargo.lock` lists `reqwest` and `hyper` because Tauri uses them on Android and iOS. They are not compiled for desktop: `cargo tree -i reqwest` prints nothing.

## 7. Files and formats

| What | Where | Format |
|---|---|---|
| Settings | `config.toml` in the OS configuration folder `twominal/`: `~/Library/Application Support` on macOS, `$XDG_CONFIG_HOME` or `~/.config` on Linux, `%APPDATA%` on Windows | TOML. One file (SET-03) |
| Command history | `history` in the OS data folder `twominal/`: `~/Library/Application Support` on macOS, `$XDG_DATA_HOME` or `~/.local/share` on Linux, `%LOCALAPPDATA%` on Windows | Plain text, one command per line, a line break written as `\n` and a backslash as `\\`. Readable only by the user (SEC-02) |
| Bundled font | `src/fonts/`, with its license (FONT-01) | The four JetBrains Mono styles |
| Shell integration | Inside the `tab` crate, as data of the Session | One script per shell, passed to the shell at startup. No user file is touched (FISH-04) |
| Webview code | `src/` | Served as it is |

Prior art in this repository: `origin/codex:src-tauri/shell-integration/` for integration that leaves the user's files alone.

## 8. Testing

- **`cargo test --workspace`** runs everything below that exists.
- **Architecture test**, `src-tauri/tests/architecture.rs`. It reads the two marked tables in this document and checks the code against them (§10).
- **Terminal Core.** A conformance suite that feeds byte streams and checks the screen (MAINT-03), and fuzz targets (REL-02). Both run with no window (ARC-03). #17.
- **Tab tier.** The `tab` crate does not depend on Tauri, so the Command Line, key handling and the Renderer's choice of text runs are unit-tested without a window.
- **Webview.** It has no logic of its own to unit-test. What it draws is checked by the acceptance scenarios (SRS §7) as documented manual tests (MAINT-02).

## 9. Risks

| Risk | Requirement | Settled by | If it fails |
|---|---|---|---|
| Canvas 2D is slow in WebKitGTK on Linux | PERF-04, PERF-05 | #48 | Draw with a WebGL2 glyph atlas behind the same `Frame` |
| A webview starts too slowly on some OS | PERF-01 | #48 | Nothing inside this design. The target or the framework is reopened |
| The three webviews draw text slightly differently | PORT-02, FONT-05 | #22, #24 | Every run is drawn at its cell's position, so differences cannot move a column |
| IPC adds too much to each key | PERF-04 | #48 | Frames while typing carry only the rows that changed. In Tauri 2.12 a channel message of up to about 8 KB of JSON goes to the webview in one step, and a larger one needs a second |
| Input methods need decisions in the webview | HW-01 | #20 | The webview half of the Input Handler may hold a composition, and nothing else |
| A webview does not follow the OS reduced-motion setting | SW-04, ACC-01 | #40 | The App Shell reads the OS setting itself and passes it to the Animator |
| A canvas cannot be read by screen readers | ACC-04 | #46 | The Renderer keeps a hidden text copy of the visible rows for them |
| A dependency of the Terminal Core needs `std` | ARC-02 | #16 | Change this document first |

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

The tests are in `src-tauri/tests/architecture.rs`.

## 11. Changing this document

- Change the document first, then the code. The two marked tables are the test's input, so the order is enforced.
- Adding a top-level component also needs a change to the SRS (ARC-01).
- Raise the version and date at the top with each change.

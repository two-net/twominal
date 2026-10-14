# Twominal — Design

**Version:** 0.11 · **Date:** 2026-10-14 · **Tracks:** [SRS](SRS.md) v0.6

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
| Renderer | Canvas 2D. Each row is drawn as runs of same-style text | FONT-04 to FONT-08, FONT-11, SW-06, HW-03 |
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
| `twominal` | A font library that lists installed monospaced families | FONT-02 | #23 |

## 3. Components

As of this version the Settings Store, the Session and the Terminal Core are built, and the App Shell opens windows, builds the native menus, gives the product its name and logo and answers the `twominal` command. Every other Rust home exists as a skeleton: types that say who owns what, and no behaviour. Of the webview homes only `src/app.js` exists. The others are created by their issues, at the paths in §1.

### Tier 1: App Shell

- **Owns** the stores, the Theme Manager, every window, the native menus and the splash screen. Each window has one Tab Manager.
- **Is the only code that names Tauri.** Every command and event is defined in `app.rs`. Commands are named `<component>_<verb>`, such as `app_ready` or `tabs_open`, and events `<component>:<what>`.
- **Wires the stores and managers.** It hears a settings change and passes it to the Theme Manager, the Tab Managers and the webviews. They never name each other.
- **Creates windows in code, not from `tauri.conf.json`.** A window then opens with the settings it needs for its first frame, so there is no flash of the wrong theme (SPL-02) and reduced motion applies from the start (SPL-07). So far that is the OS appearance the Theme setting asks for. The resolved background comes with #39 and the splash with #41.
- **Its page** hosts the tab bar, the canvas, the splash screen and the settings, About and help screens (UI-03). The tab bar is at the top and the terminal area fills the rest. Showing or hiding the tab bar takes room from the terminal area and never from the window (UI-01).
- **Shows the notices that belong to no tab**, such as a bad value in the configuration file (SET-05). They overlay the page and take no room from the terminal (UI-02).
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
| View | Theme: Dark, Light, System. Enter Full Screen | Theme: Dark, Light, System |
| Window | Minimize, Zoom, Bring All to Front, and the open windows | — |
| Help | The search field | About |

- **On macOS** the menu bar belongs to the app. On Linux and Windows each window shows it.
- **Off macOS no item takes a plain Ctrl+letter** (Appendix A). The OS's ready-made Edit items would take Ctrl+C from the shell, so there the Edit menu waits for #28.
- **Off macOS, Close Window and Quit are Twominal's own items**, because Linux has no ready-made ones.

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

### Tier 4: Terminal Core

A pure `no_std` crate. Bytes go in through one entry point. Out come the screen state, replies to terminal queries, and what shell integration reports. It holds the screen, scrollback, selection, Vim cursor and find matches. Must not depend on the interface, rendering or OS processes.

`crates/terminal-core/src/` interprets output as xterm does (TERM-01, TERM-06).

- **The parts.** `vte` splits the bytes into characters and control functions. `control.rs` says what each one does, `terminal.rs` does it to the cursor and the modes, and `screen.rs` to the rows. `width.rs` says how many cells a character takes. `report.rs` reads shell integration's reports.
- **One way in.** `TerminalCore::interpret` takes bytes and returns the replies and reports they gave, in order, one reply for each query. The Tab writes the replies to the Session (#31). A sequence cut in two by the end of one call is finished by the next, so the screen comes out the same however the output was cut (ARC-03). A character cut in two is held back by the core until the rest comes, because vte 0.15 loses the byte after such a character when another character follows it. `screen()`, `cursor()` and `modes()` say what there is to show.
- **The state, as in xterm.**
  - Two screens, primary and alternate, each made of rows that remember whether their line wrapped.
  - One cursor, which stays where it is when the screen switches. It has xterm's pending wrap: a character printed in the last column waits for the next one before the line wraps, which fish relies on. As in xterm, the wrap is pending whether autowrap is on or not, so a character printed after autowrap is turned back on wraps.
  - One saved cursor for each screen.
  - The scrolling region, the tab stops, the character sets G0 to G3, and the modes. DEC Special Graphics is there for the boxes ncurses draws.
- **What it does.**
  - The C0 controls. CAN cancels a sequence, and SUB cancels one and shows `␦`, as xterm does for a VT220.
  - IND, NEL, RI, HTS, DECSC, DECRC, RIS, DECALN, the keypad modes and the designation of character sets.
  - Moving the cursor, erasing, inserting and deleting characters and lines, scrolling, REP, the tab stops and the scrolling region.
  - The modes IRM and LNM, and the DEC private modes 1, 6, 7, 25, 47, 1047, 1048, 1049, 66, 1004, 2004, 9, 1000, 1002, 1003 and 1006. The keyboard and mouse modes are only kept, for #20 and #21 to act on.
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
- **The replies.** They hold fixed text and numbers, never bytes the program wrote (SEC-05).
  - DA1 is `CSI ? 62 ; 22 c`: a VT220 with ANSI color. fish waits for it before its first prompt. DECID (`ESC Z`) gets the same answer.
  - DA2 is `CSI > 1 ; 10 ; 0 c`. The low version keeps vim from probing for features of xterm that are not here. #21 may raise it, for vim to choose SGR mouse reports.
  - DSR: the status, and where the cursor is, in the ANSI form and the DEC one.
  - DECRQM, for the ANSI and DEC modes above but 1048, which is an action and not a state. Any other mode is reported as not known.
- **Never answered (SEC-05).** The window title and icon label reports (`CSI 20 t`, `CSI 21 t`) and every other window operation, the answerback to ENQ, and every device control string, DECRQSS and XTGETTCAP among them.
- **Ignored.** Whatever is not listed here, as ECMA-48 asks (REL-02).
  - Until their issues: SGR (#19), the cursor's shape (#25), and titles, OSC 7, color queries, the bell, links and the clipboard (#27). The kitty keyboard protocol's query is not answered, so programs know it is not there (#20).
  - Always: DECCOLM, which would resize the screen and clear it, and KAM and SRM, which would lock the keyboard or echo what is typed.
  - The C1 controls and DEL take no cell, whether a C1 control comes as UTF-8 or as a single byte. vte reads escape sequences only in their 7-bit form.
  - A control function with more than 32 parameters is ignored whole, where xterm acts on its first ones and drops the rest.
- **Where the wrap mark differs from xterm.** A row stops wrapping when an erase reaches its last cell, or when it is cleared whole. xterm's EL 0 and ECH clear the mark whatever they erase, and its EL 1 and EL 2 keep it (`util.c`, `ClearRight`, `ClearLeft`, `ClearLine`). Here EL 0 and ECH clear it only when they reach the last cell, and EL 1 and EL 2 clear it when they reach it, EL 2 always. The mark serves only copying and reflow (TERM-16, TERM-17), and a row whose end was erased does not go on in the next.
- **Until their issues.** Rows that scroll off the top are dropped. A resize cuts or pads the rows without reflowing them, and with fewer rows takes rows from the top only as far as needed to keep the cursor on the screen (#26).
- **Bounded.** `vte` has its default features off. Its buffer for an operating system command is then 256 KiB, boxed, and a longer command is cut short. Every count is clamped to the screen, so no sequence does more than a screenful of work for each of its parameters. REP, which xterm carries out in full however large its count, is cut by whole rows to no more than two screenfuls and a row: by then the characters have scrolled out every row they found, even the last columns a wide character skips, and each further row's worth leaves the screen as it was, so the screen comes out as xterm's does. A cell keeps no more than two zero-width characters. A size of zero is made one.

### What §6.1 does not name

- **Find bar, completion list, notices, Vim mode badge.** Their state belongs to the Terminal Core (find), the Command Line (completions) and the Input Handler (mode). The Renderer draws them over the terminal area from the frame (UI-02).
- **Menus and shortcuts.** A native menu item and its shortcut produce the same action. An item that acts on the app or a window, such as New Window or a theme, is carried out by the App Shell. An item that acts on a tab sends an action that the Input Handler defines (#43).

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
3. **Session.** The Tab writes input to the Session, whose writer thread passes it on, so a shell that stops reading never blocks the tab. A reader thread reads the shell's output and hands it to the tab's thread through a bounded channel: the Session calls a function the Tab hands down, and the Tab sends on its channel from it.
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
- **Shipped builds listen on no network port.** The app's files are embedded and served to the webview through Tauri's own scheme. Only `cargo tauri dev` runs a local server, and that server is part of the Tauri CLI, not of Twominal. The one thing Twominal listens on is the launcher's socket (§3): a file in a folder only the user can open, which no other machine can reach.
- **Tauri brings network crates for phones only.** `Cargo.lock` lists `reqwest` and `hyper` because Tauri uses them on Android and iOS. They are not compiled for desktop: `cargo tree -i reqwest` prints nothing.

## 7. Files and formats

| What | Where | Format |
|---|---|---|
| Settings | `config.toml` in the OS configuration folder `twominal/`: `~/Library/Application Support` on macOS, `$XDG_CONFIG_HOME` or `~/.config` on Linux, `%APPDATA%` on Windows | TOML. One file (SET-03) |
| Command history | `history` in the OS data folder `twominal/`: `~/Library/Application Support` on macOS, `$XDG_DATA_HOME` or `~/.local/share` on Linux, `%LOCALAPPDATA%` on Windows | Plain text, one command per line, a line break written as `\n` and a backslash as `\\`. Readable only by the user (SEC-02) |
| Launcher socket and lock | `twominal.sock` and `twominal.lock`, in the user's own temporary folder on macOS and in `$XDG_RUNTIME_DIR` on Linux. A debug build's are `twominal-dev.sock` and `twominal-dev.lock` | One byte for the version of the exchange, then the directory's bytes. Neither file keeps anything |
| Bundled font | `src/fonts/`, with its license (FONT-01) | The four JetBrains Mono styles |
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

The store checks only what the file can say. Whether the font is installed is the Renderer's to find out (#23), and whether the shell exists is the Session's (FISH-02).

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
- **Identity test**, `src-tauri/tests/identity.rs`. It checks the product's name, that the code spells it only as `Twominal` or `twominal`, the lowercase identifiers, and that the logo stands alone (GEN-01 to GEN-03).
- **Settings Store.** `src-tauri/src/settings/tests.rs` gives each test a configuration folder of its own, and covers loading, each kind of bad value, hand edits and saving (SET-02 to SET-05, AT-13). The store does not depend on Tauri, so it needs no window.
- **App Shell.** `src-tauri/src/app/tests.rs` covers what needs no window: window titles and labels, what each menu item does, the shortcut on each OS, and the launcher's arguments, texts and hand-over, in a folder of the test's own. `src-tauri/tests/launcher.rs` runs the executable for what it answers without starting Tauri (GEN-05). The windows and menus themselves, and a window the launcher asks for, are checked by hand (MAINT-02).
- **Session.** `crates/tab/src/session/tests.rs` starts `/bin/sh` on real pseudo-terminals and checks the bytes both ways, the environment, the directory, the size, the exit status, closing, and an exit while a background program holds the terminal (SW-01, SW-02, FISH-01, TERM-15, SEC-03). It also checks a shell that cannot start: a Shell setting that names no file while another session keeps working, a name missing from the `PATH`, a file that is not executable, and a `$SHELL` that has gone (FISH-02, AT-16). These run on macOS and Linux.
  - Shell integration is tested the same way (FISH-04 to FISH-06). zsh, bash and PowerShell start in a home folder of the test's own, with the user's files in it, and the tests check the reports, that the user's files are read in the usual order, and that the home folder is the same afterwards, file for file. Twominal's own folder, the variables it passes and the setting turned off are checked too. A test whose shell is not installed says so and passes. On macOS, `brew install powershell` installs PowerShell 7.
  - The choice of locale and of PowerShell, the waiter's rule for when to report the exit, how a program is looked for and why it cannot run, the offers and their wording, and which shells get shell integration and what it adds to their command are tested on every OS, but no shell has yet been started on Windows.
- **Terminal Core.** Everything runs with no window (ARC-03).
  - **The conformance suite** (MAINT-03), `crates/terminal-core/tests/conformance.rs`, runs the cases in `crates/terminal-core/tests/conformance/`, one file for each area of §3. A case gives a size, the bytes a program writes, and the screen, cursor, modes and replies that must follow, drawn as text. A row of the screen is the text of its cells: a wide character takes two columns of it, as in a monospace font, and a combining mark follows its character. Each expectation is what xterm does, taken from its control sequences document and its source, or a deviation this section documents, which the case's comment names. Every reply must be one the case expects, so a query that must not be answered fails if it is (SEC-05). Each case runs twice, as written and a byte at a time (ARC-03). The suite uses only the public interface, as the Tab does, and is where each TERM requirement the Terminal Core meets gets its test (MAINT-02).
  - **Unit tests**, `crates/terminal-core/src/tests.rs`, check what the public interface does not show: internal state, the reports of the shell integration, resizing, the widths, REP against printing the characters out, and that the same bytes give the same state whether they come whole, a byte at a time, cut at any one place or in random pieces (ARC-03). Characters cut in two are checked on their own. An ignored test measures plain text: `cargo test --release -p terminal-core -- --ignored` (PERF-06).
  - **Fuzzing** (REL-02). `crates/terminal-core/fuzz/` holds two cargo-fuzz targets. `interpret` gives hostile output in pieces, with resizes between, and checks the state's rules after each step and that every reply is fixed text and numbers (SEC-05). `split` checks that the output gives the same state and events whole, a byte at a time and in pieces (ARC-03). An input's first bytes choose the size, up to 160 by 60, and the cuts, so libFuzzer explores them too. `terminal.dict` gives it the pieces of sequences. What they check is `crates/terminal-core/src/fuzz.rs`, which `cargo test` also runs, on stable, with hostile output made up from the same pieces. Run each target from `crates/terminal-core` for half an hour after a change to the Terminal Core, and add whatever it finds as a test: `cargo +nightly fuzz run interpret -- -dict=fuzz/terminal.dict -timeout=5 -max_len=16384 -max_total_time=1800`, and the same for `split` with `-s none` after `split`. `-timeout` makes a slow input a hang, which REL-02 forbids. `interpret` runs under AddressSanitizer, for the `unsafe` code in vte. `split` checks logic, not memory, so it runs without: under AddressSanitizer its memory grows by about 100 MB a minute until libFuzzer stops it at 2 GB, though the Terminal Core holds none of it, and without it runs three times as fast.
  - `crates/tab/tests/programs.rs` runs fish, vim, less, `git log` and top on real pseudo-terminals through the Session, with the Terminal Core answering their queries. It checks what they show and that the screen is given back when they end (TERM-01), and that vim's cursor lands where the core put the characters of a file of wide characters, combining marks and emoji (TERM-02). Each runs in a home folder and environment of its own. A program that is not installed is said so, and its test passes.
  - `crates/tab/src/session/tests.rs` checks that the Terminal Core reads zsh's reports as the scripts write them, a list of over 60 KB among them.
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

The tests are in `src-tauri/tests/architecture.rs`.

## 11. Changing this document

- Change the document first, then the code. The two marked tables are the test's input, so the order is enforced.
- Adding a top-level component also needs a change to the SRS (ARC-01).
- Raise the version and date at the top with each change.

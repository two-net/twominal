# Twominal — Software Requirements Specification

**Version:** 0.1 (draft) · **Date:** 2026-10-13 · **Status:** For review

> This SRS is technology-agnostic. It defines what Twominal must do and how well, not how it is built. Programming languages, frameworks, libraries, rendering methods and file formats are deliberately left to the design document.

## Contents

1. [Introduction](#1-introduction)
2. [Overall description](#2-overall-description)
3. [External interfaces](#3-external-interfaces)
4. [Functional requirements](#4-functional-requirements)
5. [Non-functional requirements](#5-non-functional-requirements)
6. [Architecture](#6-architecture)
7. [Acceptance scenarios](#7-acceptance-scenarios)
8. [Traceability to the brief](#8-traceability-to-the-brief)

Appendices: [A. Default keyboard shortcuts](#appendix-a-default-keyboard-shortcuts) · [B. Settings reference](#appendix-b-settings-reference) · [C. Open questions](#appendix-c-open-questions)

## 1. Introduction

### 1.1 Purpose

This document specifies the requirements for **Twominal**, a desktop terminal application built around the fish shell. It is the reference for designing, building, testing and accepting version 1.0.

### 1.2 Scope

Twominal is a terminal emulator that runs fish in every tab. Version 1.0 provides:

- a terminal emulator that runs fish and full-screen programs correctly;
- tabs, each with its own independent fish session;
- Vim mode, for editing the fish command line and for moving through, searching and copying output;
- Dark, Light and System themes, with System as the default;
- programming-font ligatures that never break the character grid;
- short, purposeful animations that respect reduced-motion preferences;
- an animated splash screen at launch;
- a deliberately simple architecture (§6).

Twominal runs the real fish shell. It does not reimplement fish features such as autosuggestions, syntax highlighting or completions; it presents them well.

**Out of scope for 1.0:** split panes; restoring tabs after a restart; plugins and scripting; SSH or connection management; inline images; accounts, sync, telemetry and automatic updates; moving tabs between windows; Windows support (OQ-3).

### 1.3 Definitions

| Term | Meaning |
|---|---|
| fish | The friendly interactive shell; the command-line shell Twominal runs by default. |
| Session | One fish process connected to one tab through a pseudo-terminal. |
| Pseudo-terminal | The OS facility that connects a terminal application to a shell. |
| Terminal core | The part of Twominal that interprets a session's output and keeps its screen and scrollback. |
| Cell | One character position in the terminal grid. Wide characters take two cells. |
| Scrollback | Lines that have scrolled off the top of the screen and are kept for review. |
| Ligature | A single glyph that a font draws for a character sequence such as `->` or `!=`. |
| ANSI colors | The 16 palette colors (0–15) that programs use to color text. |
| Prompt Vim mode | Editing the fish command line with vi-style keys (Normal, Insert, Visual and Replace modes). |
| Scrollback Vim mode | Twominal's own mode for moving through, searching and copying output with Vim keys. |
| Shell integration | Instructions Twominal passes to fish at startup, without touching the user's files, so fish reports its working directory, prompts and Vim mode, and applies Twominal's defaults. |
| Mod | The main shortcut modifier: Cmd on macOS, Ctrl+Shift on Linux. |
| Reduced motion | The OS accessibility setting that asks apps to minimize movement. |
| Cold start | Launching Twominal when it is not already running. |

### 1.4 Conventions

- Every requirement has a stable ID, such as `TAB-04`, and is written as a "shall" statement. Retired IDs are never reused.
- **Priority:** *Must* is required for 1.0. *Should* is expected in 1.0 unless a trade-off is agreed. *Could* is included if time allows.
- Requirements apply on every supported OS unless stated otherwise. Performance targets refer to the reference machine in §5.1.

### 1.5 References

- ECMA-48, *Control Functions for Coded Character Sets*
- xterm control sequences, the de facto reference for terminal behavior
- The Unicode Standard, including UAX #11, *East Asian Width*
- WCAG 2.2, success criteria 1.4.3 *Contrast (Minimum)* and 2.3.3 *Animation from Interactions*
- fish shell documentation

## 2. Overall description

### 2.1 Product perspective

Twominal is a new, standalone desktop application. In each tab it starts fish on a pseudo-terminal, displays fish's output and passes the user's input to it.

```
┌──────────────────────────────────────────────────────────────┐
│ [ ~/code × ] [ ● vim notes.md × ] [+]                 NORMAL │
├──────────────────────────────────────────────────────────────┤
│ ~/code> git status                                           │
│ On branch main                                               │
│ nothing to commit, working tree clean                        │
│ ~/code> git log --oneline█                                   │
│                                                              │
└──────────────────────────────────────────────────────────────┘
```

Each `[ … × ]` is a tab with its close button, `●` marks new output in a background tab, `[+]` opens a new tab, and `NORMAL` is the Vim mode badge of the active (first) tab.

### 2.2 User classes

| User class | Description | Cares most about |
|---|---|---|
| Developers and power users | Spend much of the day in a terminal | Speed, tabs, shortcuts, ligatures |
| Vim users | Prefer modal, keyboard-only editing | Vim mode at the prompt and in scrollback |
| fish newcomers | New to fish or to terminals | No setup, readable defaults, discoverable settings |

### 2.3 Operating environment

- Desktop OS: macOS and Linux (Windows: OQ-3).
- fish installed on the system (OQ-1); minimum version to be decided (OQ-5).
- Standard and high-density displays, including several monitors with different scale factors.
- Any keyboard layout and input method, plus mouse and trackpad.

### 2.4 Constraints

- **C-1** This SRS prescribes no technology stack. The design document records those choices, and they must satisfy every requirement here.
- **C-2** The interface is in English for 1.0; terminal content can be any Unicode text.
- **C-3** Everything shipped with Twominal (fonts, icons, and fish if bundled) must be licensed for redistribution.

### 2.5 Assumptions and dependencies

- **A-1** fish is installed and the user can run it (unless it is bundled; OQ-1).
- **A-2** The OS reports its dark/light appearance and tells apps when it changes. Where it does not, System resolves to Dark (THM-02).
- **A-3** The OS reports its reduced-motion setting. Where it does not, Twominal assumes full motion.
- **A-4** Users who turn on Vim mode know basic Vim keys.

## 3. External interfaces

### 3.1 User interface

| ID | Requirement | Priority |
|---|---|---|
| UI-01 | The main window shall show a tab bar at the top (OQ-6) and the terminal area below it, filling the rest of the window. | Must |
| UI-02 | The find bar and notices (such as "process exited") shall overlay the terminal area without resizing the terminal grid. | Must |
| UI-03 | Twominal shall provide a settings screen, an About screen and built-in help that lists shortcuts, Vim mode keys and settings. | Must |
| UI-04 | Right-clicking the terminal shall open a menu with Copy, Paste, Select All, Find, Clear and, over a link, Open Link. | Should |
| UI-05 | Menus, window controls and shortcuts shall follow the conventions of each supported OS. | Must |
| UI-06 | No state shall be shown by color alone; activity, bell and Vim mode indicators shall also use a shape or text. | Must |

### 3.2 Hardware interfaces

| ID | Requirement | Priority |
|---|---|---|
| HW-01 | Every key and modifier combination that is not a Twominal shortcut shall reach the session. Input methods and dead keys shall work, with the text being composed shown at the cursor. | Must |
| HW-02 | Mouse and trackpad shall support selection, high-precision scrolling, opening links, and mouse reporting to programs that request it. | Must |
| HW-03 | Text shall render sharply at any display scale and re-render correctly when a window moves to a monitor with a different scale. | Must |

### 3.3 Software interfaces

| ID | Requirement | Priority |
|---|---|---|
| SW-01 | **fish:** Twominal shall start one fish process per tab as an interactive login shell, connected through a pseudo-terminal. | Must |
| SW-02 | **Environment:** each session shall receive `TERM=xterm-256color`, `COLORTERM=truecolor`, `TERM_PROGRAM=Twominal`, `TERM_PROGRAM_VERSION=<version>`, and a UTF-8 locale if none is set. | Must |
| SW-03 | **Appearance:** Twominal shall read the OS dark/light appearance and react when it changes. | Must |
| SW-04 | **Reduced motion:** Twominal shall read the OS reduced-motion setting and react when it changes. | Must |
| SW-05 | **Clipboard:** copy and paste shall use the OS clipboard. On Linux, selecting text shall also set the primary selection, and middle-click shall paste it. | Must |
| SW-06 | **Fonts:** Twominal shall use installed fonts, falling back to other installed fonts for characters the chosen font lacks. | Must |
| SW-07 | **Links:** links shall open in the OS default handler for their type (SEC-07). | Should |

### 3.4 Communication interfaces

None. Twominal itself opens no network connections (SEC-01); programs running inside it use the network as usual.

## 4. Functional requirements

### 4.1 Product identity

| ID | Requirement | Priority |
|---|---|---|
| GEN-01 | The product shall be named **Twominal** in all user-facing text: window titles, menus, About screen, splash screen, installer and help. | Must |
| GEN-02 | The lowercase form `twominal` shall be used for the launcher command, the configuration folder and other identifiers. | Must |
| GEN-03 | One Twominal logo shall be used for the app icon, the About screen and the splash screen, and shall be legible on dark and light backgrounds. | Must |
| GEN-04 | The window title shall show the active tab's title. | Must |
| GEN-05 | The `twominal` command shall open a new window, optionally in a given directory, and support `--help` and `--version`. | Should |

### 4.2 Terminal emulation

| ID | Requirement | Priority |
|---|---|---|
| TERM-01 | Twominal shall emulate an xterm-compatible terminal (ECMA-48 control functions plus common xterm extensions) well enough that fish and common full-screen programs, such as editors, pagers, system monitors and version-control tools, display and behave correctly. | Must |
| TERM-02 | Text shall be UTF-8. Wide characters (UAX #11) shall take two cells, combining marks shall join their base character, and emoji shall render without misaligning the grid. | Must |
| TERM-03 | Twominal shall support the 16-color palette, the 256-color palette and 24-bit color, for text and background. | Must |
| TERM-04 | Twominal shall support bold, dim, italic, underlined, struck-through, inverse and hidden text. | Must |
| TERM-05 | Twominal shall support double, curly, dotted and dashed underlines, and a separate underline color. | Should |
| TERM-06 | Twominal shall support the alternate screen, scrolling regions, line wrapping, inserting and deleting characters and lines, tab stops, and saving, restoring, showing and hiding the cursor. | Must |
| TERM-07 | The cursor shall take the shape programs request (block, bar or underline, blinking or steady), and otherwise the shape set in settings. | Must |
| TERM-08 | Key input shall cover printable keys, modifiers, and function and navigation keys. Esc shall be sent immediately, with no added delay. | Must |
| TERM-09 | Twominal shall support the progressive keyboard enhancement protocol, so programs that request it can tell apart ambiguous keys, such as Ctrl+I and Tab, or Esc and Alt combinations. | Should |
| TERM-10 | Twominal shall report clicks, drags and wheel events, with extended coordinates, to programs that request mouse reporting. Holding Shift shall bypass reporting so the user can still select text. | Must |
| TERM-11 | Twominal shall support bracketed paste and focus in/out reporting. | Must |
| TERM-12 | Programs shall be able to set the tab title, report the working directory, query the current theme colors and ring the bell. | Must |
| TERM-13 | Programs shall be able to create hyperlinks and write to the clipboard (subject to SEC-04). | Should |
| TERM-14 | Each tab shall keep 10,000 lines of scrollback by default, configurable from 0 to 100,000. | Must |
| TERM-15 | When the terminal area changes size, the session shall be told the new number of rows and columns. | Must |
| TERM-16 | On resize, wrapped lines shall reflow to the new width. | Should |
| TERM-17 | Dragging shall select characters, double-clicking a word and triple-clicking a line; Option+drag on macOS and Ctrl+drag on Linux shall select a rectangular block. Copying shall return the original text, with trailing spaces removed and wrapped lines joined. | Must |
| TERM-18 | Find (Mod+F) shall highlight every match on screen and in scrollback, move between matches, and offer a case-sensitivity toggle. | Must |
| TERM-19 | Find shall support regular expressions. | Could |
| TERM-20 | URLs in output shall be detected. Cmd+click on macOS or Ctrl+click on Linux shall open them, and hovering shall show the full target. | Should |
| TERM-21 | The bell shall be visual by default (a short flash) and configurable as audible or off. | Should |
| TERM-22 | Clear (Mod+K) shall clear the screen and scrollback, keeping the line with the cursor. | Should |

### 4.3 fish integration

fish is the heart of Twominal. Twominal starts it, lets it report what it is doing, and never touches the user's own fish configuration.

| ID | Requirement | Priority |
|---|---|---|
| FISH-01 | Each new tab shall start fish (SW-01). A window's first tab shall start in the home directory, or in the directory given to the launcher. | Must |
| FISH-02 | If fish cannot be found or fails to start, the tab shall explain the problem, show how to install fish, and offer to open the OS default shell instead. The rest of Twominal shall keep working. | Must |
| FISH-03 | The user shall be able to choose another shell executable and its arguments. If that shell is not fish, fish-specific features (Prompt Vim mode, shell integration) shall turn off quietly and everything else shall keep working. | Should |
| FISH-04 | Shell integration shall be applied when a session starts, without creating, changing or deleting any of the user's fish files. The user's own fish configuration shall load as usual. | Must |
| FISH-05 | Shell integration shall report to Twominal the session's working directory, where each prompt starts and ends, when the first prompt is ready, and fish's current key-binding mode. | Must |
| FISH-06 | Shell integration shall be switchable off in settings. Features that depend on it (new tab in the same directory, prompt jumps, the Vim mode badge, detecting the first prompt) shall then fall back or hide without errors. | Should |
| FISH-07 | Unless the user has customized fish's colors, shell integration shall make fish use only the 16 ANSI colors for syntax highlighting, autosuggestions and the completion pager, so fish's colors follow Twominal's theme. | Should |

### 4.4 Tabs

| ID | Requirement | Priority |
|---|---|---|
| TAB-01 | Each window shall hold one or more tabs. Each tab shall own exactly one session, with its own screen, scrollback, find state and Vim mode state. | Must |
| TAB-02 | Mod+T, the `+` button and the menu shall open a new tab at the end of the tab bar and make it active. | Must |
| TAB-03 | A new tab shall start in the active tab's working directory, or in the home directory if that directory is unknown or the user chose Home in settings. | Should |
| TAB-04 | Mod+W, the tab's close button and a middle-click on the tab shall close it. | Must |
| TAB-05 | Before closing a tab, a window or the app while an affected tab runs a program other than the idle shell, Twominal shall ask for confirmation and name the running programs. The confirmation can be turned off in settings. | Must |
| TAB-06 | When the shell exits with status 0, its tab shall close. With any other status, the tab shall stay open, show the status, and offer Restart and Close. | Must |
| TAB-07 | Closing the last tab shall close its window. Closing the last window shall follow the OS convention; on macOS the app may keep running. | Must |
| TAB-08 | The user shall switch tabs by clicking them, or with shortcuts for the next, previous, first to eighth and last tab (Appendix A). | Must |
| TAB-09 | Switching tabs shall show the chosen tab's content immediately; only the tab bar may animate. | Must |
| TAB-10 | Tabs shall be reorderable by drag and drop. | Must |
| TAB-11 | Tabs shall be movable left and right with shortcuts. | Should |
| TAB-12 | A tab's title shall default to the title its session reports (by default, fish reports the running command and the current directory). The user may rename a tab by double-clicking it or with a shortcut; the custom name stays until the user clears it. | Must |
| TAB-13 | A background tab shall show one indicator when new output arrives and a different one when the bell rings; both clear when the tab is shown. | Should |
| TAB-14 | When tabs no longer fit, they shall shrink to a minimum width and then the tab bar shall scroll; the active tab shall always be fully visible. | Must |
| TAB-15 | Mod+N shall open a new window with one tab. | Should |

### 4.5 Vim mode

Vim mode has two parts:

- **Prompt Vim mode** — vi-style editing of the fish command line. fish provides the editing; Twominal switches it on, shows the current mode and keeps it responsive. The *Vim mode* setting controls this part.
- **Scrollback Vim mode** — Twominal's own mode for moving through, searching and copying output with Vim keys instead of the mouse. It is always available.

#### Prompt Vim mode

| ID | Requirement | Priority |
|---|---|---|
| VIM-01 | When the Vim mode setting is On (default Off; OQ-2), new fish sessions shall start with fish's vi key bindings. When it is Off, Twominal shall leave fish's key bindings as the user's configuration sets them. | Must |
| VIM-02 | Changing the setting shall apply to new tabs immediately and to open tabs at their next prompt. | Should |
| VIM-03 | Unless the user's fish configuration sets its own cursor shapes, the cursor shall show the mode: a block in Normal and Visual, a bar in Insert, and an underline in Replace. | Must |
| VIM-04 | While fish waits at its prompt, a badge at the right end of the tab bar shall show the active tab's mode: NORMAL, INSERT, VISUAL or REPLACE. The badge shall hide while another program runs. | Should |
| VIM-05 | Twominal shall add no delay to Esc (TERM-08); with fish's default escape timeout, the switch to Normal mode shall be visible within 50 ms of pressing Esc. | Must |

#### Scrollback Vim mode

| ID | Requirement | Priority |
|---|---|---|
| VIM-06 | Ctrl+Shift+Space shall turn Scrollback Vim mode on and off for the active tab. `q` and `i` shall also exit it; `Esc` shall clear the selection, or exit if nothing is selected. | Must |
| VIM-07 | While the mode is on, keys shall not reach the session, the badge shall show SCROLLBACK (SCROLLBACK · VISUAL while selecting), and a Vim cursor, styled differently from the terminal cursor, shall start at the terminal cursor's position. | Must |
| VIM-08 | These motions shall work, each with an optional count such as `5j`: `h` `j` `k` `l`, `w` `b` `e` `W` `B` `E`, `0` `^` `$`, `gg` `G`, `H` `M` `L`, and Ctrl+U, Ctrl+D, Ctrl+B, Ctrl+F. | Must |
| VIM-09 | `{` and `}` shall move between blank-line-separated blocks and `%` to the matching bracket; `*` and `#` shall search for the word under the cursor. | Should |
| VIM-10 | `/` and `?` shall search forward and backward through the screen and scrollback, highlight every match and wrap around at the ends; `n` and `N` shall go to the next and previous match. | Must |
| VIM-11 | `v`, `V` and Ctrl+V shall start a character, line or block selection; `y` shall copy the selection to the OS clipboard, clear it and stay in Scrollback Vim mode. | Must |
| VIM-12 | `[[` and `]]` shall jump to the previous and next prompt (needs shell integration). | Could |
| VIM-13 | `gx` or Enter shall open the link under the Vim cursor (SEC-07). | Could |
| VIM-14 | While the mode is on, new output shall not move the view; a "new output" notice shall appear instead. Exiting shall return to the live bottom of the screen. | Must |
| VIM-15 | Scrollback Vim mode shall work in every tab, whatever program is running and whatever the Vim mode setting. | Must |

### 4.6 Themes

| ID | Requirement | Priority |
|---|---|---|
| THM-01 | Twominal shall offer three themes: **Dark**, **Light** and **System**. The default shall be **System**. | Must |
| THM-02 | With System selected, Twominal shall follow the OS appearance. When the OS appearance changes, Twominal shall switch within 1 s, without restarting sessions or losing screen or scrollback content. If the OS does not report an appearance, System shall resolve to Dark. | Must |
| THM-03 | A theme change shall apply at once to every window and tab, the tab bar, settings, help, the splash screen and the terminal colors. | Must |
| THM-04 | Each theme shall define the background, foreground, cursor, text under the cursor, selection, find highlights, the 16 ANSI colors, the tab bar and tab states, and the Vim mode badge. | Must |
| THM-05 | In each theme, contrast against the background shall be at least 7:1 for the default foreground, 4.5:1 for ANSI colors 1–6 and 9–14, and 3:1 for ANSI color 8, which is used for dim text such as autosuggestions. | Must |
| THM-06 | Theme changes shall cross-fade in at most 300 ms (subject to ANIM-02). | Should |
| THM-07 | The chosen theme shall persist across restarts and be switchable from settings and from the View menu. | Must |
| THM-08 | Programs that subscribe to color-scheme changes shall be notified when the theme switches between dark and light. | Could |
| THM-09 | Individual theme colors shall be overridable in the configuration file. | Could |

### 4.7 Fonts and ligatures

| ID | Requirement | Priority |
|---|---|---|
| FONT-01 | Twominal shall bundle a default monospaced font that has programming ligatures and is licensed for redistribution (OQ-4). | Must |
| FONT-02 | The user shall be able to choose any installed monospaced font and a size from 6 to 72 pt (default 13 pt), and change the size with the zoom shortcuts (Appendix A). | Must |
| FONT-03 | Ligatures shall be On by default and switchable in settings; a change shall apply immediately to every tab. | Must |
| FONT-04 | With ligatures On, Twominal shall draw every ligature the font provides, such as `->`, `=>`, `!=`, `>=`, `===` and `::`. Fonts without ligatures shall render normally. | Must |
| FONT-05 | A ligature shall cover exactly the cells of the characters it replaces. Ligatures shall never shift text or change column positions. | Must |
| FONT-06 | Ligatures shall form only between characters in the same row that have the same colors and attributes. | Must |
| FONT-07 | When the terminal cursor or the Vim cursor is on any character of a ligature, that ligature shall be drawn as separate characters so the cursor position is clear. | Must |
| FONT-08 | Selecting part of a ligature shall highlight only the selected cells, and copying shall always return the original characters. | Must |
| FONT-09 | Bold and italic text shall use the font's own bold and italic styles when they exist. | Should |
| FONT-10 | Box-drawing and block characters shall join without gaps between cells, and prompt symbols such as powerline glyphs shall line up with the grid. | Should |
| FONT-11 | Characters missing from the chosen font shall be drawn from fallback fonts and fit the cell grid. | Must |
| FONT-12 | Individual font features, such as stylistic sets, shall be switchable in the configuration file. | Could |

### 4.8 Animation

Animation exists to make changes easy to follow. It must never slow the user down.

| ID | Requirement | Priority |
|---|---|---|
| ANIM-01 | Twominal shall animate the splash intro and exit; tabs opening, closing and reordering; the active-tab indicator; theme changes; Vim mode badge changes; the find bar appearing and disappearing; and the visual bell. | Must |
| ANIM-02 | An Animations setting (On/Off, default On) shall control all animations. When it is On and the OS asks for reduced motion, Twominal shall use no movement or scaling, only cross-fades of at most 150 ms or instant changes. | Must |
| ANIM-03 | Interface animations other than the splash intro shall last 100–300 ms and use easing rather than linear motion. | Must |
| ANIM-04 | Animations shall never delay input or output: keystrokes reach the session and output is drawn while animations run. An interrupted animation shall continue smoothly from its current state. | Must |
| ANIM-05 | Terminal output shall never animate: new text appears at once, without fading or sliding. | Must |
| ANIM-06 | Trackpad scrolling shall move smoothly, pixel by pixel. | Should |
| ANIM-07 | While Twominal is idle, nothing shall animate except the cursor blink. In an unfocused window, the cursor shall not blink and shall show as an outline. | Must |
| ANIM-08 | Two cursor effects shall be available, both off by default: a fading blink, and a smooth glide between positions. | Could |

### 4.9 Splash screen

| ID | Requirement | Priority |
|---|---|---|
| SPL-01 | On cold start, Twominal shall show a splash screen with the Twominal logo and name and an intro animation of at most 800 ms. | Must |
| SPL-02 | The splash shall use the resolved theme from its first frame, with no flash of the wrong background. | Must |
| SPL-03 | The first window and session shall start loading while the splash plays; the splash shall not hold back startup work. | Must |
| SPL-04 | The splash shall end once its intro has finished and the first session is ready: its first prompt is shown or, with shell integration off, its first output has arrived. Including its exit transition, the splash shall never last longer than 1.5 s; after that the window appears even if fish is still starting. | Must |
| SPL-05 | Any key press or click shall end the splash at once. Every key typed during the splash, including the one that ended it, shall reach the first session, in order. | Must |
| SPL-06 | The splash shall hand over to the main window with a transition of at most 250 ms. | Should |
| SPL-07 | With reduced motion, the splash shall be static and end as soon as the first session is ready. | Must |
| SPL-08 | A setting shall turn the splash off (default On). | Must |
| SPL-09 | The splash shall not appear for new windows or tabs, or when switching back to a running Twominal. | Must |

### 4.10 Settings

| ID | Requirement | Priority |
|---|---|---|
| SET-01 | A settings screen (Mod+,) shall cover every setting in Appendix B. | Must |
| SET-02 | Changes shall take effect immediately, without restarting Twominal or any session, except where Appendix B says otherwise. | Must |
| SET-03 | Settings shall be stored in one human-readable plain-text file, in the OS's standard per-user configuration location, in a folder named `twominal`. | Must |
| SET-04 | Saving a hand-edited configuration file shall apply the changes live. | Should |
| SET-05 | A missing, unreadable or invalid configuration shall never stop Twominal from starting: each bad value shall fall back to its default, and Twominal shall show a notice naming the setting and the problem. | Must |
| SET-06 | Keyboard shortcuts shall be remappable in the configuration file, and conflicting mappings shall be reported. | Should |
| SET-07 | The settings screen shall offer "Reset to defaults". | Should |

## 5. Non-functional requirements

### 5.1 Performance

Targets are measured on a reference machine: a laptop with a 4-core CPU from 2021 or later, 8 GB of RAM, integrated graphics and a 60 Hz display. A "usable prompt" is visible and accepts input. Startup times exclude time spent in the user's own fish configuration.

| ID | Measure | Target | Priority |
|---|---|---|---|
| PERF-01 | Cold start to usable prompt, splash off | ≤ 500 ms | Must |
| PERF-02 | Cold start to usable prompt, splash on | ≤ 1.5 s | Must |
| PERF-03 | New tab to usable prompt | ≤ 200 ms | Must |
| PERF-04 | Key press to updated frame while typing at the prompt | Median ≤ 16 ms; 95th percentile ≤ 33 ms | Must |
| PERF-05 | Frame rate during animations and scrolling | ≥ 60 fps | Must |
| PERF-06 | Throughput of plain-text output | ≥ 25 MB/s | Should |
| PERF-07 | Responsiveness during heavy output | Shortcuts and tab switches respond within 100 ms; Ctrl+C takes visible effect within 250 ms | Must |
| PERF-08 | CPU use while idle, with the cursor blinking | ≤ 1% of one core | Must |
| PERF-09 | Memory with one tab | ≤ 200 MB | Should |
| PERF-10 | Memory per extra tab with full default scrollback | ≤ 30 MB | Should |
| PERF-11 | Scale | With 50 tabs open, the active tab still meets every target above | Should |

### 5.2 Reliability

| ID | Requirement | Priority |
|---|---|---|
| REL-01 | A shell or program that crashes, hangs or floods output in one tab shall not affect other tabs or the app. | Must |
| REL-02 | Malformed, unknown or hostile control sequences shall never crash or hang Twominal; unknown sequences shall be ignored. The terminal core shall be fuzz-tested. | Must |
| REL-03 | Changing theme, font, ligature or animation settings, or resizing the window, shall never lose screen or scrollback content, apart from lines pushed past the scrollback limit. | Must |

### 5.3 Usability and accessibility

| ID | Requirement | Priority |
|---|---|---|
| USE-01 | A new user shall be able to install Twominal, open it and run a fish command without any setup. | Must |
| USE-02 | Every feature shall be usable from the keyboard alone. | Must |
| USE-03 | Default shortcuts shall follow OS conventions and shall not take keys that fish uses for line editing (Appendix A). | Must |
| ACC-01 | Twominal shall honor the OS reduced-motion setting, including changes while it runs (ANIM-02, SPL-07). | Must |
| ACC-02 | Both bundled themes shall meet THM-05, and interface text shall have a contrast of at least 4.5:1. | Must |
| ACC-03 | Tabs, buttons, menus and settings shall be exposed to OS screen readers with meaningful names and states, and shall show a visible keyboard focus indicator. | Must |
| ACC-04 | OS screen readers shall be able to read terminal text, including the current line and new output. | Should |

### 5.4 Security and privacy

| ID | Requirement | Priority |
|---|---|---|
| SEC-01 | Twominal itself shall make no network connections: no telemetry, analytics, crash reports or update checks in 1.0. | Must |
| SEC-02 | Twominal shall not write terminal input or output to disk. | Must |
| SEC-03 | Sessions shall run with the user's own privileges; Twominal shall never ask for elevated privileges. | Must |
| SEC-04 | Programs may write to the clipboard (default Allow) but not read it (default Deny); each can be set to Allow, Ask or Deny. | Must |
| SEC-05 | Twominal shall not implement control sequences that send attacker-controllable text back as input, such as window-title reporting. | Must |
| SEC-06 | Pasting text with line breaks into a program that has not enabled bracketed paste shall ask for confirmation; the warning can be turned off. | Should |
| SEC-07 | Links shall open only through an explicit user action and, by default, only for `http`, `https`, `mailto` and `file` links. The full target shall be visible before the user opens it, for example on hover. | Must |

### 5.5 Portability

| ID | Requirement | Priority |
|---|---|---|
| PORT-01 | Version 1.0 shall run on the two most recent major macOS releases and on mainstream Linux desktops, under both X11 and Wayland. | Must |
| PORT-02 | Behavior shall be the same on every supported OS, except where that OS's conventions differ, such as shortcuts, menus and file locations. | Must |

### 5.6 Maintainability

| ID | Requirement | Priority |
|---|---|---|
| MAINT-01 | The design shall follow the architecture in §6. | Must |
| MAINT-02 | Every Must requirement shall be verified by an automated test or a documented manual test. | Must |
| MAINT-03 | The terminal core shall have an automated conformance suite that feeds it byte streams and checks the resulting screen state. | Must |

## 6. Architecture

"Simple architecture" here means a handful of components with one job each, dependencies that point one way, and tabs that are fully isolated from each other. The structure below is logical and implies no particular technology.

### 6.1 Components

```
App Shell
├── Settings Store
├── Theme Manager
├── Animator
└── Tab Manager
    └── Tab (one per tab, isolated)
        ├── Session
        ├── Terminal Core
        ├── Input Handler
        └── Renderer
```

| Component | Responsibility | Must not |
|---|---|---|
| App Shell | Starts and stops the app; owns windows, menus and the splash screen | Contain terminal logic |
| Settings Store | Loads, validates, saves and announces settings | — |
| Theme Manager | Turns Dark, Light or System into the active palette and announces changes | — |
| Animator | Runs every interface animation under one policy (Animations setting, reduced motion) | Delay input or output |
| Tab Manager | Creates, orders, renames and closes tabs | Reach into a tab's session or screen |
| Session | Starts fish on a pseudo-terminal with shell integration; carries bytes both ways; reports exit | Interpret output |
| Terminal Core | Interprets output; keeps the screen, scrollback, selection and Vim cursor; answers terminal queries | Depend on the interface, rendering or OS processes |
| Input Handler | Turns keys and mouse events into shortcuts, encoded input or Vim mode actions | Draw anything |
| Renderer | Draws the Terminal Core's state with the active font, ligatures and palette | Change terminal state |

### 6.2 Data flow in a tab

```
keys ──► Input Handler ──► Session ◄──► fish
               │            ▲   │
               │    replies │   │ output
               │            │   ▼
               └─ Vim ──► Terminal Core ──► Renderer ──► window
```

1. The Input Handler receives keys and mouse events. Shortcuts go to the Tab Manager or App Shell. In Scrollback Vim mode, keys move the view, the Vim cursor and the selection in the Terminal Core. All other input is encoded and sent to the Session.
2. The Session passes input to fish, and fish's output to the Terminal Core.
3. The Terminal Core interprets the output, updates the screen and scrollback, and answers terminal queries through the Session.
4. The Renderer draws the Terminal Core's state with the active font, ligatures and palette.

### 6.3 Architecture requirements

| ID | Requirement | Priority |
|---|---|---|
| ARC-01 | Twominal shall consist of the components in §6.1, each with the single responsibility listed there. Adding a top-level component requires updating this SRS. | Must |
| ARC-02 | Dependencies shall point one way: from the App Shell, to the managers, to the tab components. The Terminal Core shall not depend on the interface, the Renderer or OS process facilities. | Must |
| ARC-03 | The Terminal Core shall be deterministic and testable without a window: the same input bytes and grid size shall always produce the same screen state. | Must |
| ARC-04 | Tabs shall share nothing mutable. A tab owns its Session, Terminal Core, input state and render state, and receives settings and theme only through change notifications. | Must |
| ARC-05 | Each cross-cutting concern shall live in one place: settings in the Settings Store, theme resolution in the Theme Manager, animation policy in the Animator, and fish-specific behavior in the Session's shell integration. | Must |
| ARC-06 | Version 1.0 shall contain no plugin system, scripting engine or network component. | Must |
| ARC-07 | The design document shall keep a one-page overview of these components and their interfaces, updated along with the code. | Should |

## 7. Acceptance scenarios

Each scenario starts from default settings unless it says otherwise.

| ID | Scenario | Expected result | Covers |
|---|---|---|---|
| AT-01 | First launch while the OS is in dark mode | Splash on a dark background, then a Dark-themed window with a fish prompt, within 1.5 s | SPL-01–04, THM-01–02, PERF-02 |
| AT-02 | Type `ls` during the splash | The splash closes on the first key, and `ls` appears at the prompt | SPL-05 |
| AT-03 | Switch the OS to light mode while an editor runs in one tab | Within 1 s Twominal cross-fades to Light; the editor keeps running and scrollback is intact | THM-02, THM-06, REL-03 |
| AT-04 | Type `a != b -> c` with ligatures on, put the cursor on `!=`, then copy the line | `!=` and `->` are drawn as ligatures; `!=` splits under the cursor; the clipboard holds `a != b -> c` | FONT-04, FONT-05, FONT-07, FONT-08 |
| AT-05 | Open three tabs, run `cd /tmp` in the third, then open a fourth | The fourth tab starts in `/tmp`; the tab bar animates; switching tabs is instant | TAB-02–03, TAB-09, ANIM-01 |
| AT-06 | Drag the third tab to the first position, then press the shortcut for tab 1 | The order updates with an animation, and the moved tab becomes active | TAB-08, TAB-10 |
| AT-07 | Run `vim` in a tab and press Mod+W | A confirmation names `vim`; Cancel keeps the tab open | TAB-05 |
| AT-08 | Turn Vim mode on, open a tab, type `echo hello`, press Esc, then `0` | The badge changes from INSERT to NORMAL and the cursor from bar to block within 50 ms; the cursor jumps to the line start | VIM-01, VIM-03–05 |
| AT-09 | Run `seq 1 5000`, press Ctrl+Shift+Space, then type `/4242`, Enter, `V`, `y`, `q` | The match is highlighted, the line `4242` is copied, and the view returns to the live bottom | VIM-06–07, VIM-10–11, VIM-14 |
| AT-10 | Turn on the OS reduced-motion setting and relaunch | The splash is static; tab and theme changes have no movement, only fades of 150 ms or less, or instant changes | ANIM-02, SPL-07, ACC-01 |
| AT-11 | Launch Twominal on a machine without fish | The tab explains how to install fish and offers the OS default shell | FISH-02 |
| AT-12 | Print a 100 MB text file, then press Ctrl+C | The interface stays responsive, and output stops within 250 ms | PERF-06–07 |
| AT-13 | Save an invalid value in the configuration file | A notice names the setting, its default is used, and Twominal keeps running | SET-04–05 |

## 8. Traceability to the brief

| Brief item | Covered by |
|---|---|
| Application like a terminal + fish | §4.2 (TERM), §4.3 (FISH), SW-01–02 |
| Named Twominal | §4.1 (GEN) |
| Animation | §4.8 (ANIM), SPL-01, SPL-06, THM-06, PERF-05, ACC-01 |
| Dark / light / system, system as default | §4.6 (THM); THM-01 makes System the default |
| Font ligatures | §4.7 (FONT) |
| Simple architecture | §6 (ARC), §5.6 (MAINT) |
| Splash screen | §4.9 (SPL) |
| Tabs | §4.4 (TAB) |
| Vim mode | §4.5 (VIM) |

## Appendix A. Default keyboard shortcuts

In the requirements, Mod means Cmd on macOS and Ctrl+Shift on Linux. All shortcuts can be remapped (SET-06). Twominal never takes a plain Ctrl+letter combination, so fish's editing keys keep working.

### Keyboard

| Action | macOS | Linux |
|---|---|---|
| New tab | Cmd+T | Ctrl+Shift+T |
| Close tab | Cmd+W | Ctrl+Shift+W |
| New window | Cmd+N | Ctrl+Shift+N |
| Next tab | Cmd+Shift+] or Ctrl+Tab | Ctrl+Page Down or Ctrl+Tab |
| Previous tab | Cmd+Shift+[ or Ctrl+Shift+Tab | Ctrl+Page Up or Ctrl+Shift+Tab |
| Go to tab 1–8 | Cmd+1 … Cmd+8 | Alt+1 … Alt+8 |
| Go to last tab | Cmd+9 | Alt+9 |
| Move tab left / right | Cmd+Shift+← / Cmd+Shift+→ | Ctrl+Shift+Page Up / Ctrl+Shift+Page Down |
| Rename tab | Cmd+Shift+I | Ctrl+Shift+I |
| Copy / paste | Cmd+C / Cmd+V | Ctrl+Shift+C / Ctrl+Shift+V |
| Select all | Cmd+A | Ctrl+Shift+A |
| Find | Cmd+F | Ctrl+Shift+F |
| Clear screen and scrollback | Cmd+K | Ctrl+Shift+K |
| Font bigger / smaller / reset | Cmd+= / Cmd+- / Cmd+0 | Ctrl+Shift+= / Ctrl+Shift+- / Ctrl+Shift+0 |
| Settings | Cmd+, | Ctrl+Shift+, |
| Scrollback Vim mode on/off | Ctrl+Shift+Space | Ctrl+Shift+Space |
| Scroll one page up / down | Shift+Page Up / Shift+Page Down | Shift+Page Up / Shift+Page Down |

### Mouse

| Action | macOS | Linux |
|---|---|---|
| Open link | Cmd+click | Ctrl+click |
| Rectangular selection | Option+drag | Ctrl+drag |
| Select while a program uses the mouse | Shift+drag | Shift+drag |
| Rename tab | Double-click the tab | Double-click the tab |
| Close tab | Middle-click the tab | Middle-click the tab |
| Paste the primary selection | — | Middle-click in the terminal |

### Scrollback Vim mode keys

| Keys | Action |
|---|---|
| `h` `j` `k` `l` | Left, down, up, right |
| `w` `b` `e` | Next word, previous word, end of word |
| `W` `B` `E` | The same, for space-separated words |
| `0` `^` `$` | Start of line, first non-blank character, end of line |
| `gg` `G` | Top of scrollback, bottom |
| `H` `M` `L` | Top, middle and bottom of the visible screen |
| Ctrl+U, Ctrl+D | Half a page up, down |
| Ctrl+B, Ctrl+F | A page up, down |
| `{` `}` | Previous, next block of lines (Should) |
| `%` | Matching bracket (Should) |
| `/` `?` | Search forward, backward |
| `n` `N` | Next, previous match |
| `*` `#` | Search for the word under the cursor, forward or backward (Should) |
| `v` `V` Ctrl+V | Select characters, lines, block |
| `y` | Copy the selection |
| `[[` `]]` | Previous, next prompt (Could) |
| `gx` or Enter | Open the link under the cursor (Could) |
| `Esc` | Clear the selection, or exit if nothing is selected |
| `q` `i` Ctrl+Shift+Space | Exit |
| A count, then a motion | Repeat the motion, e.g. `5j` |

## Appendix B. Settings reference

| Setting | Values | Default | Takes effect |
|---|---|---|---|
| Theme | Dark, Light, System | **System** | Immediately |
| Font | Any installed monospaced font | Bundled font | Immediately |
| Font size | 6–72 pt | 13 pt | Immediately |
| Font ligatures | On, Off | On | Immediately |
| Vim mode | On, Off | Off (OQ-2) | New tabs; open tabs at their next prompt |
| Animations | On, Off | On, reduced when the OS asks | Immediately |
| Splash screen | On, Off | On | Next launch |
| Cursor style | Block, Bar, Underline | Block | Immediately; programs may override |
| Cursor blink | On, Off | On | Immediately |
| Scrollback | 0–100,000 lines | 10,000 | Immediately; shrinking drops the oldest lines |
| Shell | Executable and arguments | fish, as an interactive login shell | New tabs |
| Shell integration | On, Off | On | New tabs |
| New tab directory | Same as current tab, Home | Same as current tab | Immediately |
| Confirm before closing running programs | On, Off | On | Immediately |
| Bell | Visual, Audible, Off | Visual | Immediately |
| Clipboard writes by programs | Allow, Ask, Deny | Allow | Immediately |
| Clipboard reads by programs | Allow, Ask, Deny | Deny | Immediately |
| Multi-line paste warning | On, Off | On | Immediately |
| Keyboard shortcuts | Remappable in the configuration file | Appendix A | Immediately |

## Appendix C. Open questions

| ID | Question | Proposed answer |
|---|---|---|
| OQ-1 | Should Twominal bundle fish or require it to be installed? Bundling means zero setup, but Twominal would then have to ship fish updates and comply with fish's GPL-2.0 license. | Use the installed fish, and guide the user when it is missing (FISH-02). |
| OQ-2 | Should Vim mode be On by default? | Off; Vim users turn it on once. |
| OQ-3 | Is Windows in scope? fish runs on Windows only through WSL, Cygwin or MSYS2. | Not in 1.0. |
| OQ-4 | Which font should be bundled? It needs programming ligatures and a license that allows redistribution. | Decide during design. |
| OQ-5 | What is the oldest supported fish version? | The oldest version that supports everything shell integration needs. |
| OQ-6 | Should the tab bar hide when a window has only one tab? | No; always show it in 1.0. |

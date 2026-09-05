# Twominal

Twominal is a fast, cross-platform terminal application built with Rust, Tauri
v2, React, TypeScript, and xterm.js. The current Milestone 6 build provides real
native PTYs, independent terminal tabs, persistent settings, configurable
terminal typography and ligatures, automatic light/dark and solar appearance,
accessible motion, Unicode, ANSI color, and prompt-safe history, suggestions,
completions, and Vim-style command-line editing.

Twominal does **not** install, bundle, invoke, or require Fish Shell. Its
Fish-inspired history, suggestions, and completion features are implemented by
Twominal and work on a machine where Fish has never been installed.

<p align="center">
  <img src="assets/screenshot.png" alt="Twominal startup screen" width="800" />
  <br /><br />
  <img src="assets/screenshot-terminal.png" alt="Twominal terminal showing executable completions" width="800" />
</p>

## Prerequisites

- Node.js 22.13 or newer (22.x) and npm 10.9
- The stable Rust toolchain, including Cargo
- The [Tauri v2 system prerequisites](https://v2.tauri.app/start/prerequisites/)
  for your platform

On macOS, install the Xcode command-line tools. On Windows, install the Microsoft
C++ Build Tools and WebView2 requirements described by Tauri. On Debian or
Ubuntu, install Tauri's native development packages:

``` sh
sudo apt update
sudo apt install libwebkit2gtk-4.1-dev \
  build-essential \
  curl \
  wget \
  file \
  libxdo-dev \
  libssl-dev \
  libayatana-appindicator3-dev \
  librsvg2-dev \
  patchelf
```

## Develop

Install the exact JavaScript dependency set from the project root:

``` sh
npm ci
```

Start Vite and the native Tauri application together:

``` sh
npm run tauri dev
```

Create a release build and platform bundle with:

``` sh
npm run tauri build
```

The Tauri CLI is a locked project dependency, so a global Tauri installation is
not needed.

## Architecture and current status

The React frontend owns window layout, appearance, and the xterm.js renderer.
It transfers terminal bytes over a narrow, typed Tauri IPC interface. The Rust
backend detects a safe default shell and owns each shell process and PTY through
`portable-pty`, which uses native Unix PTYs on macOS/Linux and ConPTY on Windows.
PTY reads, writes, waits, and bounded output flow control stay off the UI thread.

``` text
src/
├── app/                 React application, appearance, and window chrome
├── config/              typed settings model and narrow native client
├── settings/            accessible settings dialog and controls
├── shell/               prompt state, history, suggestions, tokenization, completion
├── tabs/                tab reducer, shortcuts, and accessible tab strip
├── terminal/            xterm renderer, byte transport, input queue, themes
├── theme/               system and sunrise/sunset theme scheduling
├── ui/                  startup splash
├── vim/                 prompt-scoped Vim input state machine
├── windows/             native window creation and tab-transfer coordination
└── performance/         deterministic interactive-path performance budgets
src-tauri/
├── capabilities/        app-window capability allowlist
├── permissions/         exact terminal, config, history, and completion permissions
├── shell-integration/   bundled semantic markers for zsh, bash, and PowerShell
└── src/
    ├── commands.rs      validated Tauri IPC boundary
    ├── completion.rs    bounded PATH, filesystem, and environment discovery
    ├── history.rs       private, bounded, atomic command-history persistence
    ├── logging.rs       bounded, privacy-preserving local lifecycle/error log
    ├── pty.rs           portable native PTY/ConPTY adapter
    ├── shell.rs         default-shell detection and launch plan
    ├── storage.rs       cross-platform atomic private-file replacement
    └── terminal/        session ownership, lifecycle, and flow control
```

The core runtime dependencies are deliberately small:

- Tauri v2 provides the native application shell and narrowly scoped IPC.
- `portable-pty` supplies Unix PTYs and Windows ConPTY behind one Rust API.
- xterm.js performs terminal emulation; its fit, Unicode 11, and optional WebGL
  addons handle sizing, character widths, and accelerated rendering.
- React and Vite provide the minimal window UI and reproducible frontend build.
- `crossbeam-channel` provides bounded, blocking-worker communication without
  moving PTY I/O onto the UI thread.

Milestones 1 through 6 include:

- A real PTY-backed shell with UTF-8 input, ANSI output, control keys, and resize
- Safe cross-platform shell detection and startup in the user's home directory
- A bounded 10,000-line terminal scrollback and WebGL rendering with fallback
- System, light, dark, and sunset-to-sunrise appearance modes
- Process-exit reporting, restart, cleanup, and startup-error recovery
- Independent PTY-backed tabs that remain mounted while inactive
- New, close, switch, numeric-select, and reorder tab actions
- Live tab dragging into a new native window or another Twominal window without
  restarting the shell or losing its terminal transcript
- Sanitized shell/OSC tab titles and deterministic neighbor selection on close
- A recoverable zero-tab state and a bounded 20-tab workspace
- Persistent, validated JSON settings behind two narrowly scoped native commands
- Live font family, size, line height, letter spacing, weight, and ligature controls
- Local solar calculations with optional manual coordinates and an 18:00–06:00
  fallback; Twominal never requests or transmits location
- Subtle splash, tab, settings, and theme transitions with both an application
  motion toggle and `prefers-reduced-motion` support
- Per-session authenticated prompt detection for zsh, bash, PowerShell, and
  `pwsh`, with a safe no-enhancement fallback for other shells
- Private, deduplicated, 1,000-entry command history with prefix navigation and
  explicit clearing in Settings
- Recency-ranked history autosuggestions accepted with Right Arrow or Ctrl+F
- Modular executable, filesystem, and environment-variable completion with a
  keyboard- and mouse-accessible menu
- Tolerant shell token classification for commands, arguments, options, quoted
  text, paths, operators, and environment references
- Immediate enhancement suspension after command submission, on untracked line
  edits, away from the bottom viewport, and in the alternate screen buffer
- Configurable, prompt-scoped Vim Insert and Normal modes with a live footer
  indicator and Unicode/grapheme-safe cursor motion
- Vim motions, changes, deletion, undo/redo, and filtered history navigation
  implemented independently of Fish or the launched shell's own editing mode
- A generic React recovery screen that does not expose caught error details
- Structured local lifecycle/error logging with bounded rotation and no terminal
  content, command, path, environment value, or session identifier collection
- Bounded, parallel PTY-child shutdown on tab close, reload, window destruction,
  normal application exit, and Rust stack unwinding
- Large-input, bounded-queue, Vim-editing, suggestion, and completion performance
  budgets with a dedicated repeatable test command
- Locked frontend/Rust validation and native builds configured for macOS,
  Windows, and Linux, plus unsigned QA packaging and tagged draft GitHub
  Releases with checksums

## Tab controls

| Action | macOS | Windows/Linux |
| --- | --- | --- |
| New tab | Command+T | Ctrl+T |
| Close active tab | Command+W | Ctrl+W |
| Select tabs 1–9 | Command+1–9 | Ctrl+1–9 |
| Previous/next tab | Command+Shift+`[` / `]` | Ctrl+Shift+`[` / `]` |
| Open settings | Command+`,` | Ctrl+`,` |

Tabs can also be activated and closed with the tab strip, reordered by dragging,
or moved from keyboard focus with Alt+Shift+Left/Right. Plain Left/Right moves
focus through the tab list and wraps at either end. Drag a running tab outside
all Twominal windows to detach it when at least one tab will remain in the source
window. Drop it on another Twominal window's tab strip to combine the windows;
the source window closes if no tabs remain there.

## Fish-inspired shell experience

Twominal adds semantic prompt markers to supported shells; it does not replace
the shell or parse arbitrary screen output. Every terminal session receives a
random nonce. The bundled integration removes that nonce from the exported
environment before user commands run, and the frontend accepts prompt markers
only when they contain the matching nonce.

Automatic prompt integration currently supports zsh, bash, Windows PowerShell,
and `pwsh`. User startup files still run. `cmd.exe`, nushell, and other shells
remain fully functional PTY sessions, but Twominal enhancements stay unavailable
until a safe integration is implemented for that shell.

At an authenticated, reliably tracked prompt:

- Type a history prefix to see its most recent continuation in subdued text;
  accept it with Right Arrow or Ctrl+F.
- Use Up/Down to move through commands matching the text already typed. Moving
  forward past the newest match restores the original draft.
- Use Tab to accept one completion or open multiple matches. Ctrl+Space opens
  the completion menu explicitly; Up/Down selects, Enter/Right/Tab accepts, and
  Escape closes it.
- Executable names come from `PATH`, file and directory names come from the
  authenticated prompt working directory, and environment completion returns
  names only—never values. POSIX and PowerShell variable syntax is preserved;
  the formatter is also ready for cmd syntax when prompt integration lands.
- The footer reports the current syntax context without repainting or delaying
  terminal output.

Prompt enhancements turn off as soon as Enter submits the command and remain
off while the command or another terminal application owns input. Suggestions
and completion overlays also hide in the alternate screen, when viewing
scrollback, or after input Twominal cannot track exactly. This keeps applications
such as Vim, tmux, ssh, less, fzf, and top on the ordinary PTY input path.

Twominal history is separate from shell history. A command beginning with
whitespace, containing control/bidirectional formatting characters, exceeding
4 KiB, or spanning multiple lines is never stored. Repeated commands update one
entry. History can be permanently cleared from Settings with an explicit
two-step confirmation.

## Vim command-line editing

Enable **Vim-style command editing** in Settings → Keyboard. Every new verified
prompt begins in Insert mode. The `i`, `a`, `A`, and `I` commands enter Insert
mode, while Escape (or Ctrl+`[`) returns to Normal mode. The footer shows the
active mode. The setting applies immediately to all open
tabs and persists across restarts. Mode-changing keys are consumed completely
and never appear in the command line.

Normal mode supports:

| Purpose | Keys |
| --- | --- |
| Enter Insert mode | `i`, `a`, `A`, `I` |
| Move | `h`, `l`, `w`, `b`, `e`, `0`, `$` |
| Matching history | `k`, `j` |
| Delete | `x`, `dd`, `D` |
| Change | `cw`, `ciw` |
| Undo / redo | `u`, Ctrl+`r` |

Insert sessions are grouped into one undo step, and cursor/edit operations honor
extended grapheme clusters such as emoji with modifiers and combining marks.
Paste and IME commits are accepted in Insert mode and ignored in Normal mode.
Control keys such as Ctrl+C and Enter still pass to the PTY. Normal-mode routing
remains active while viewing scrollback so its commands cannot become literal
shell input. Vim handling is disabled outside an authenticated, reliably tracked
prompt, while commands run, and in the alternate screen. It therefore does not
capture keystrokes intended for Vim/Neovim, ssh, tmux, less, fzf, or other
terminal applications.

## Settings

Settings are stored as human-readable `config.json` in the platform-native
per-user application configuration directory. The frontend cannot choose this
path or access arbitrary files. Writes are bounded, validated, normalized, and
atomically replaced; Unix files are created with mode `0600`.

The settings window contains only working controls:

- Appearance: Follow System, Light, Dark, or Sunset to Sunrise
- Optional latitude/longitude for local astronomical calculations
- Terminal font family, size, line height, letter spacing, weight, and ligatures
- Interface animations, with the operating system's reduced-motion preference
  always taking priority
- Vim-style prompt editing, applied live to every open terminal tab
- Saved command count and confirmed local-history clearing

No fonts are bundled or enumerated. Ligatures use xterm's character-joiner API
and the chosen installed font; disabling them updates all open tabs immediately.

## Security boundary

The frontend cannot choose an executable or submit a native command string. It
can only start the detected shell, send bounded raw terminal input, resize its
own opaque session ID, acknowledge rendered output, close that session,
transfer it through a one-time token bound to a specific Twominal window,
load/save fixed-schema local data, and request bounded completion candidates for
an owned running session. Completion discovery never executes a candidate and
environment values never cross IPC. Every command validates webview ownership,
input size, session state, terminal dimensions, or completion bounds as
applicable. All scripts are bundled locally under a strict CSP; there are no
generic shell, process, or filesystem plugin permissions.

Tab drag data contains only an opaque drag ID, a window label, and a local tab
ID. The source window validates that live drag before creating a
destination-bound transfer token; PTY session IDs and transfer tokens never
enter drag data.

Tauri's optional `freezePrototype` switch is disabled because xterm.js requires
mutable prototype state during module initialization. The compensating controls
are a local-only script policy, locked dependencies, no runtime code loading,
eight terminal commands, two fixed-schema configuration commands, and four exact
history/completion commands.

## Diagnostics and cleanup

Twominal writes `twominal-events.jsonl` in Tauri's platform-native per-user
application log directory. It is a deliberately small diagnostic stream: each
line contains a timestamp, severity, stable event name, and—only where
applicable—a stable error code or aggregate active-session count. It never
records commands, terminal input or output, working directories, shell paths,
environment names or values, session identifiers, or caught frontend error
text.

The active log rotates before it exceeds 1 MiB and keeps one backup named
`twominal-events.jsonl.1`. On Unix, the directory is mode `0700` and files are
mode `0600`; Windows uses the current user's application-data ACLs. Logging is
best-effort and uses a non-blocking lock so a diagnostic write cannot deadlock a
panic path or terminal shutdown.

Every PTY child has a dedicated supervisor completion signal. Closing several
tabs requests termination for all children before waiting, with one shared
two-second cleanup deadline rather than a separate delay per tab. Page reload,
window destruction, normal process exit, and unwind cleanup all drain the
session registry. A frontend render failure displays a generic reload action;
reloading starts the normal owner cleanup without presenting private error
details in the webview.

## Packaging and release boundary

`npm run tauri build` creates the native release bundle for the current
platform. Release metadata is checked independently with:

``` sh
npm run check:metadata
```

The manually dispatched `Unsigned packages` workflow produces short-lived QA
artifacts on all three platform runners: Debian and AppImage packages on Linux,
a DMG on macOS, and NSIS/MSI installers on Windows. The AppImage is placed in a
tar archive so its executable bit survives artifact download. Artifact names
include the commit SHA, retention is 14 days, updater artifacts are disabled,
and the packaging jobs have read-only repository permission.

Pushing a tag such as `v0.1.0` runs the `Release` workflow. The tag must be `v`
followed by the exact version shared by `package.json`, `Cargo.toml`, and
`tauri.conf.json`. After every platform package succeeds, one least-privileged
job collects the five installers, adds `SHA256SUMS.txt`, and creates a draft
GitHub Release. A failed platform build cannot create a partial release, and a
rerun may replace assets only while the release remains a draft.

The automated packages remain intentionally unsigned. Before publishing the
draft, public distribution still requires project-owned identities and
credentials: Apple Developer ID signing and notarization for macOS, an
Authenticode identity for Windows, and the project's chosen package/repository
signing policy for Linux. No placeholder identity, secret, or updater key is
committed to the repository. Replace the staging assets with those signed
packages and regenerate `SHA256SUMS.txt` before publishing the draft.

To prepare a release, update the version in all three manifests and their lock
files, pass CI and the manual compatibility checklist, commit the result, then
push the matching tag:

``` sh
version="$(node -p "require('./package.json').version")"
git tag -a "v${version}" -m "Twominal ${version}"
git push origin "v${version}"
```

## Quality checks

Run the same checks used in CI:

``` sh
npm run typecheck
npm run lint
npm test
npm run test:performance
npm run build
npm run check:metadata
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features --locked -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --all-features --locked
npm run tauri -- build --debug --no-bundle
```

The CI workflow is configured to run this suite on Ubuntu 22.04, macOS 14, and
Windows Server 2022 with pinned Node.js and Rust versions. A successful run on
all three platforms is required, but it does not replace manual PTY and
terminal-application testing on each operating system. In this workspace, the
macOS native build, app/DMG bundle, default-zsh launch, and child cleanup have
been exercised locally; Linux and Windows runtime validation must be recorded
from their respective machines or CI runners.

## Manual compatibility checklist

Run the applicable checks on macOS, Windows, and Linux before declaring a
platform validated. Record the shell, operating-system version, and result for
each run.

- [ ] Launches the expected default shell: bash and zsh on Unix; Windows
  PowerShell, `pwsh`, and `cmd.exe` where installed
- [ ] Plain text, pasted text, UTF-8, emoji, combining characters, CJK wide
  characters, and installed Nerd Font glyphs display without corruption
- [ ] ANSI 16/256 colors, truecolor, cursor movement, alternate screen, and clear
  screen sequences render correctly
- [ ] Ctrl+C interrupts a foreground command without closing Twominal
- [ ] Ctrl+D sends end-of-file where supported; Ctrl+Z suspends a foreground Unix
  process where supported
- [ ] At zsh, bash, PowerShell, and `pwsh` prompts, the footer reports the active
  command/token context; unsupported shells show no enhancement state
- [ ] A typed history prefix shows a subdued continuation; Right Arrow and Ctrl+F
  accept it without duplicating or dropping characters
- [ ] Up/Down filters saved history by the current prefix and restores the exact
  original draft after the newest match
- [ ] Tab and Ctrl+Space complete executables, relative/absolute paths (including
  spaces), directories, Unicode names, and environment-variable names
- [ ] Completion uses the new working directory after `cd`, does not reveal
  environment values, and never executes a candidate
- [ ] Commands beginning with whitespace, multiline pastes, and aborted or
  untrackable input are absent from Twominal history
- [ ] With Vim editing enabled, the footer begins in INSERT at each verified
  prompt; `i`, `a`, `A`, and `I` enter INSERT and Escape/Ctrl+`[` returns to
  NORMAL at the expected cursor position
- [ ] Normal-mode `h/l/w/b/e/0/$`, `x`, `dd`, `D`, `cw`, `ciw`, `u`, and Ctrl+`r`
  preserve cursor placement for ASCII, emoji, combining marks, and wide text
- [ ] Normal-mode `k`/`j` navigate matching Twominal history, while Ctrl+C,
  Ctrl+D, Ctrl+Z, and Enter retain their ordinary terminal behavior
- [ ] Vim-mode keys pass through unchanged after command submission and inside
  ssh, Vim/Neovim, tmux, top/htop, less, fzf, and alternate-screen programs
- [ ] Clear history requires confirmation, removes all suggestions immediately,
  and remains empty after an application restart
- [ ] Resizing and maximizing the window updates terminal rows/columns without
  wrapping corruption or noticeable input lag
- [ ] `ssh` connects, accepts interactive input, resizes, and exits back to the
  local shell cleanly
- [ ] `vim` or `nvim`, and `nano`, accept normal control/navigation input and
  restore the terminal after exit
- [ ] `tmux` starts, creates/switches panes, receives resize events, and detaches
  cleanly
- [ ] `top`/`htop`, `less`, and `fzf` use the alternate screen and receive input
  normally
- [ ] Suggestions and completions disappear while `ssh`, Vim/Neovim, tmux,
  top/htop, less, fzf, and other foreground or full-screen programs own input
- [ ] Exiting or interrupting a foreground program restores prompt enhancements
  without capturing that program's keystrokes as command history
- [ ] Large output (for example `seq 1 100000` or PowerShell `1..100000`) remains
  responsive and scrollback stays bounded
- [ ] Rapid output followed by typing preserves byte order and does not freeze the
  UI or grow memory without bound
- [ ] New tabs start independent shell processes and keep separate working
  directories, screen contents, and scrollback
- [ ] Mouse and keyboard tab switching preserve foreground programs and input
  focus; numeric and previous/next shortcuts select the intended tab
- [ ] Closing an inactive or active tab terminates only its PTY and selects the
  right neighbor when possible, then the left neighbor
- [ ] Drag and Alt+Shift+Left/Right reorder tabs without restarting their sessions
- [ ] Dragging a tab outside all Twominal windows works only when another tab
  remains; the new window preserves the foreground process, title, screen, and input
- [ ] Dropping a tab on another Twominal tab strip inserts and activates it, and
  closes the source window only when no tabs remain there
- [ ] A rejected or failed tab transfer leaves the source tab and PTY running
- [ ] Closing the final tab leaves a usable empty state; New Terminal and the new
  tab shortcut start a fresh session
- [ ] OSC shell titles update only their owning tab and control/bidirectional
  characters cannot enter the visible title
- [ ] Exiting the shell reports its status; Restart opens a fresh shell; closing
  the window leaves no orphaned shell process
- [ ] Light, Dark, and System modes update the whole window; System follows an OS
  appearance change
- [ ] Sunset-to-sunrise mode changes at calculated local transitions when valid
  coordinates are set and uses 18:00–06:00 when coordinates are absent
- [ ] Changing font family, size, line height, letter spacing, and weight updates
  all tabs and sends corrected PTY dimensions without restarting their shells
- [ ] Ligatures toggle without shifting terminal cell boundaries; Unicode, wide
  characters, combining characters, and cursor placement remain correct
- [ ] Settings survive a complete application restart, malformed configuration
  reports a safe error, and Restore Defaults produces the documented values
- [ ] Settings open with Command/Ctrl+`,`; keyboard focus remains trapped in the
  dialog and returns to its previous control when the dialog closes
- [ ] Startup has no artificial delay, and reduced-motion mode removes or
  simplifies the splash transition
- [ ] Disabling interface animations removes splash, tab, settings, and theme
  motion without delaying terminal rendering
- [ ] With WebGL unavailable or after context loss, terminal rendering continues
  through xterm.js's fallback renderer
- [ ] Closing a window with several active tabs terminates every owned shell and
  returns within the shared cleanup deadline without leaving child processes
- [ ] The diagnostic JSONL contains only documented fields, rotates at 1 MiB,
  retains at most one backup, and has the expected per-user permissions
- [ ] The dedicated performance suite passes repeatedly on the target hardware;
  large input remains ordered and pending input/scrollback stay bounded
- [ ] The platform's unsigned QA installer installs, launches, and uninstalls
  cleanly; separately signed release candidates pass native trust verification

## Milestone status

Milestones 1–6 are implemented. GitHub Actions builds tagged versions and
creates a draft GitHub Release. Publishing remains an operator step because
platform signing/notarization identities and target-machine manual compatibility
results are external to the source tree. Do not publish an unsigned draft
without completing those checks.

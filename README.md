# Twominal

A desktop terminal with a fish-style command line built in. [SRS.md](SRS.md) says what it must do, and [DESIGN.md](DESIGN.md) says how it is built.

Twominal is a Tauri v2 app. One Rust process holds all state and logic, and one OS webview per window draws. The webview code is plain HTML and ES modules (no bundler, no Node dependencies).

Version 1.0 is being built. So far the Settings Store works, the App Shell opens windows and builds the native menus, the product has its name and logo, the `twominal` command opens windows, the Session runs the user's shell on a pseudo-terminal with shell integration for zsh, bash and PowerShell, the Terminal Core interprets what it writes as xterm does, wide characters, combining marks, emoji, colors and text styles included, and the other components exist as a skeleton. No window shows a shell yet: that comes with tabs.

## Layout

| Path | What |
|---|---|
| `src-tauri/` | The app: the App Shell, the stores and the managers |
| `crates/tab/` | Everything one tab owns |
| `crates/terminal-core/` | The Terminal Core |
| `src/` | The webview's files, served as they are |

DESIGN.md §1 lists every component and its home.

## Development

```sh
cargo tauri dev
```

## Test

```sh
cargo test --workspace
```

This includes the architecture test, which checks the code against DESIGN.md, and the Terminal Core's conformance suite, whose cases are in `crates/terminal-core/tests/conformance/`.

## Fuzz

The Terminal Core is fuzzed with cargo-fuzz, which needs the nightly toolchain. Nothing else does. Run each target for half an hour after changing the Terminal Core:

```sh
rustup toolchain install nightly
cargo install cargo-fuzz
cd crates/terminal-core
cargo +nightly fuzz run interpret -- -dict=fuzz/terminal.dict -timeout=5 -max_len=16384 -max_total_time=1800 -rss_limit_mb=4096
cargo +nightly fuzz run split -s none -- -dict=fuzz/terminal.dict -timeout=5 -max_len=16384 -max_total_time=1800
```

A crash or a hang is saved in `fuzz/artifacts/`. DESIGN.md §8 says what each target checks.

## Build

```sh
cargo tauri build
```

## Command

The app's executable is also the `twominal` command. After a build it is `target/release/twominal`, and inside the macOS bundle `Twominal.app/Contents/MacOS/twominal`.

```sh
twominal              # a new window
twominal DIRECTORY    # a new window whose first tab starts in DIRECTORY
twominal --help
twominal --version
```

If Twominal is already running, the window opens there and the command returns at once. No shell starts in the directory until tabs exist. DESIGN.md §3 has the details.

## Logo

`src/logo.svg` is the one logo. The app icons in `src-tauri/icons/` are generated from it, so after changing it run:

```sh
cargo tauri icon src/logo.svg
```

# Twominal

A desktop terminal with a fish-style command line built in. [SRS.md](SRS.md) says what it must do, and [DESIGN.md](DESIGN.md) says how it is built.

Twominal is a Tauri v2 app. One Rust process holds all state and logic, and one OS webview per window draws. The webview code is plain HTML and ES modules (no bundler, no Node dependencies).

Version 1.0 is being built. So far the Settings Store works and the other components exist as a skeleton.

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

This includes the architecture test, which checks the code against DESIGN.md.

## Build

```sh
cargo tauri build
```

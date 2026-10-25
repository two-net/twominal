# Twominal

A Tauri v2 desktop app with a Rust backend (`src-tauri/`) and a plain HTML/CSS/JavaScript frontend (`src/`). Building and running it needs no Node toolchain.

## Develop

```sh
cargo tauri dev
```

## Build

```sh
cargo tauri build
```

## Test

```sh
cd src-tauri && cargo test
node --test tests/js/*.test.js
```

The JavaScript tests use Node's built-in test runner with no packages. They live outside `src/` because everything in `src/` ships inside the app.

## Dependencies

Each dependency is kept on purpose (SRS ARC-07):

| Crate | Why |
|---|---|
| `tauri`, `tauri-build` | Windows, menus, webview and IPC. |
| `serde`, `serde_json` | Data sent to the frontend and the app state file. |
| `alacritty_terminal` | Terminal Core: escape-sequence parser, grid, scrollback, reflow and damage tracking, plus the pseudo-terminal and its I/O loop. |
| `libc` | macOS process queries for window titles (foreground process, its name and directory). Already a dependency of Tauri. |
| `muda` (tests only) | Checks that every default shortcut parses. Already a dependency of Tauri. |

## Recommended IDE Setup

- [VS Code](https://code.visualstudio.com/) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)

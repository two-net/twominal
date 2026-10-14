//! Launcher test (SRS §4.1).
//!
//! Runs the `twominal` command for what it answers by itself: the help, the version and its
//! problems (GEN-05). None of them starts Tauri, so the test needs no display. A command that
//! opens a window is checked by hand (DESIGN.md §8).

use std::process::{Command, Output};

fn twominal(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_twominal"))
        .args(arguments)
        .output()
        .expect("the command runs")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn version_prints_the_name_and_the_version() {
    for option in ["--version", "-V"] {
        let output = twominal(&[option]);
        assert_eq!(output.status.code(), Some(0));
        // The version is the one in `tauri.conf.json`, which the package has too.
        assert_eq!(
            text(&output.stdout),
            format!("Twominal {}\n", env!("CARGO_PKG_VERSION"))
        );
        assert_eq!(text(&output.stderr), "");
    }
}

#[test]
fn help_prints_the_usage() {
    for option in ["--help", "-h"] {
        let output = twominal(&[option]);
        assert_eq!(output.status.code(), Some(0));
        let help = text(&output.stdout);
        for line in [
            "Usage: twominal [DIRECTORY]",
            "  -h, --help",
            "  -V, --version",
        ] {
            assert!(help.contains(line), "{help}");
        }
        assert_eq!(text(&output.stderr), "");
    }
}

#[test]
fn an_unknown_option_is_refused() {
    let output = twominal(&["--nope"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        text(&output.stderr),
        "twominal: unknown option '--nope'\nTry 'twominal --help'.\n"
    );
    assert_eq!(text(&output.stdout), "");
}

#[test]
fn a_second_directory_is_refused() {
    let output = twominal(&["one", "two"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(text(&output.stderr).starts_with("twominal: more than one directory: 'two'\n"));
}

#[test]
fn a_directory_that_is_not_there_is_refused() {
    let missing = std::env::temp_dir().join("twominal-no-such-directory");
    let output = twominal(&[missing.to_str().expect("the folder's name is text")]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        text(&output.stderr),
        format!("twominal: no such directory: {}\n", missing.display())
    );
    assert_eq!(text(&output.stdout), "");
}

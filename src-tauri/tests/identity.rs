//! Identity test (SRS §4.1).
//!
//! Checks that the product has one name, one lowercase form of it for identifiers, and one
//! logo (GEN-01 to GEN-03). DESIGN.md §3 says where each of them is kept.

use std::fs;
use std::path::{Path, PathBuf};

/// The product's name as the user reads it (GEN-01).
const NAME: &str = "Twominal";

/// The form every identifier uses (GEN-02).
const IDENTIFIER: &str = "twominal";

/// The one logo (GEN-03).
const LOGO: &str = "src/logo.svg";

/// Where the name is written: the webview's files, the Rust sources, the manifests, the
/// configuration and the README.
const WRITTEN_IN: [&str; 7] = [
    "README.md",
    "crates",
    "src",
    "src-tauri/Cargo.toml",
    "src-tauri/capabilities",
    "src-tauri/src",
    "src-tauri/tauri.conf.json",
];

// ---------------------------------------------------------------------------------------
// The tests
// ---------------------------------------------------------------------------------------

#[test]
fn the_product_is_named_twominal() {
    // Window titles, the menus, the About dialog and the installers all take it from here.
    let config = read("src-tauri/tauri.conf.json");
    assert_eq!(
        json_string(&config, "productName").as_deref(),
        Some(NAME),
        "`productName` in src-tauri/tauri.conf.json must be {NAME} (GEN-01)"
    );
    assert!(
        read("src/index.html").contains(&format!("<title>{NAME}</title>")),
        "the page in src/index.html must be titled {NAME} (GEN-01)"
    );
}

#[test]
fn the_name_is_spelled_one_way() {
    let mut problems = Vec::new();

    for place in WRITTEN_IN {
        for file in files(place) {
            // A file that is not text, such as a font, spells nothing.
            let Ok(text) = fs::read_to_string(root().join(&file)) else {
                continue;
            };
            for spelling in misspellings(&text) {
                problems.push(format!("{file} spells it `{spelling}`"));
            }
        }
    }

    assert_none(
        problems,
        "the name is `Twominal` to the user and `twominal` in identifiers (GEN-01, GEN-02)",
    );
}

#[test]
fn identifiers_use_the_lowercase_name() {
    // The package names the executable, and so the launcher command (GEN-05). The folder of
    // the configuration file is checked with the Settings Store.
    let executable = Path::new(env!("CARGO_BIN_EXE_twominal"));
    assert_eq!(
        executable.file_stem().and_then(|name| name.to_str()),
        Some(IDENTIFIER)
    );

    let config = read("src-tauri/tauri.conf.json");
    assert!(
        json_string(&config, "mainBinaryName").is_none_or(|name| name == IDENTIFIER),
        "`mainBinaryName` must not rename the executable (GEN-02)"
    );
    let identifier = json_string(&config, "identifier").expect("the bundle has an identifier");
    assert!(
        identifier.rsplit('.').next() == Some(IDENTIFIER) && identifier == identifier.to_lowercase(),
        "the bundle identifier `{identifier}` must be lowercase and end in `.{IDENTIFIER}` (GEN-02)"
    );
}

#[test]
fn the_logo_stands_alone() {
    let logo = read(LOGO);
    let mut problems = Vec::new();

    // The icon generator takes a square.
    match view_box(&logo) {
        Some([.., width, height]) if width == height => {}
        Some(_) => problems.push("is not square".to_string()),
        None => problems.push("has no `viewBox`".to_string()),
    }
    // What a webview and the icon generator could draw differently, or would have to load.
    for (written, what) in [
        ("<text", "text, which needs a font"),
        ("<style", "a style sheet"),
        ("<script", "a script"),
        ("<image", "another image"),
        ("<foreignObject", "HTML"),
        ("href", "a reference"),
    ] {
        if logo.contains(written) {
            problems.push(format!("holds {what} (`{written}`)"));
        }
    }
    // `url(#mark)` names a gradient in the file. Any other `url(` leaves it.
    if logo.matches("url(").count() != logo.matches("url(#").count() {
        problems.push("loads something with `url(`".to_string());
    }

    assert_none(
        problems,
        &format!("{LOGO} is drawn alike everywhere and loads nothing (GEN-03, ARC-06)"),
    );
}

// ---------------------------------------------------------------------------------------
// Reading the repository
// ---------------------------------------------------------------------------------------

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the app crate sits one level below the repository root")
        .to_path_buf()
}

fn read(file: &str) -> String {
    fs::read_to_string(root().join(file)).unwrap_or_else(|error| panic!("{file}: {error}"))
}

/// `place` itself if it is a file, or every file under it, as sorted repository paths.
fn files(place: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut pending = vec![place.to_string()];
    while let Some(path) = pending.pop() {
        match fs::read_dir(root().join(&path)) {
            Ok(entries) => pending.extend(
                entries
                    .flatten()
                    .map(|entry| format!("{path}/{}", entry.file_name().to_string_lossy())),
            ),
            Err(_) => found.push(path),
        }
    }
    found.sort();
    found
}

/// Every spelling of the name in `text` that is neither the name nor the identifier.
fn misspellings(text: &str) -> Vec<&str> {
    // Lowering ASCII letters moves nothing, so a place in one is the same place in the other.
    text.to_ascii_lowercase()
        .match_indices(IDENTIFIER)
        .map(|(start, found)| &text[start..start + found.len()])
        .filter(|&spelling| spelling != NAME && spelling != IDENTIFIER)
        .collect()
}

/// The four numbers of an SVG's `viewBox`: left, top, width and height.
fn view_box(svg: &str) -> Option<[f64; 4]> {
    let value = svg.split_once("viewBox=\"")?.1.split_once('"')?.0;
    let numbers: Vec<f64> = value
        .split([' ', ','])
        .filter(|number| !number.is_empty())
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    numbers.try_into().ok()
}

/// The value of a string field in JSON text, or `None` if the field is absent or not a string.
fn json_string(json: &str, key: &str) -> Option<String> {
    let after = &json[json.find(&format!("\"{key}\""))? + key.len() + 2..];
    let value = after.trim_start().strip_prefix(':')?.trim_start();
    let value = value.strip_prefix('"')?;
    Some(value[..value.find('"')?].to_string())
}

fn assert_none(problems: Vec<String>, rule: &str) {
    assert!(
        problems.is_empty(),
        "\n{rule}:\n  - {}\n",
        problems.join("\n  - ")
    );
}

// ---------------------------------------------------------------------------------------
// The checks check themselves
// ---------------------------------------------------------------------------------------

#[test]
fn misspellings_sees_every_other_spelling() {
    assert!(misspellings("Twominal, twominal_lib and two Twominals").is_empty());
    assert_eq!(
        misspellings("TwoMinal sets TWOMINAL_HOME"),
        ["TwoMinal", "TWOMINAL"]
    );
}

#[test]
fn view_box_reads_its_four_numbers() {
    assert_eq!(
        view_box(r#"<svg viewBox="0 0 1024 1024">"#),
        Some([0.0, 0.0, 1024.0, 1024.0])
    );
    assert_eq!(view_box(r#"<svg viewBox="0,0,16">"#), None);
    assert_eq!(view_box("<svg>"), None);
}
